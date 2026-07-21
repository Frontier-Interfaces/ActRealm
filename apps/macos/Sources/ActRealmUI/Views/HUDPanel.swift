import AppKit
import Combine
import ActRealmKit
import SwiftUI

enum HUDPanelLayout {
    static let topSpacing: CGFloat = 8

    static func origin(
        screenFrame: NSRect,
        visibleFrame: NSRect,
        safeAreaInsets: NSEdgeInsets,
        panelSize: NSSize
    ) -> NSPoint {
        let safeMinX = max(screenFrame.minX + safeAreaInsets.left, visibleFrame.minX)
        let safeMaxX = min(screenFrame.maxX - safeAreaInsets.right, visibleFrame.maxX)
        let centeredX = screenFrame.midX - panelSize.width / 2
        let availableWidth = safeMaxX - safeMinX
        let originX: CGFloat
        if availableWidth >= panelSize.width {
            originX = min(max(centeredX, safeMinX), safeMaxX - panelSize.width)
        } else {
            originX = centeredX
        }

        // `safeAreaInsets.top` excludes a MacBook camera housing/notch. The
        // visible frame also excludes an exposed menu bar, so using the lower
        // of the two top edges keeps the capsule clear of both.
        let safeTop = min(
            visibleFrame.maxY,
            screenFrame.maxY - safeAreaInsets.top
        )
        let minimumY = screenFrame.minY + safeAreaInsets.bottom
        let originY = max(minimumY, safeTop - panelSize.height - topSpacing)
        return NSPoint(x: originX, y: originY)
    }
}

struct HUDDisplayOption: Identifiable, Equatable {
    let id: UInt32
    let name: String
    let isMain: Bool
}

enum HUDDisplayCatalog {
    static var options: [HUDDisplayOption] {
        let mainID = CGMainDisplayID()
        return NSScreen.screens.compactMap { screen in
            guard let id = displayID(for: screen) else { return nil }
            return HUDDisplayOption(id: id, name: screen.localizedName, isMain: id == mainID)
        }
        .sorted { left, right in
            if left.isMain != right.isMain { return left.isMain }
            return left.name.localizedStandardCompare(right.name) == .orderedAscending
        }
    }

    static var mainScreen: NSScreen? {
        screen(with: CGMainDisplayID()) ?? NSScreen.main ?? NSScreen.screens.first
    }

    static func displayID(for screen: NSScreen) -> UInt32? {
        (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
            .uint32Value
    }

    static func screen(with id: UInt32) -> NSScreen? {
        NSScreen.screens.first { displayID(for: $0) == id }
    }
}

enum HUDDisplaySelection {
    static func resolve(
        mode: HUDDisplayMode,
        selectedID: UInt32?,
        selectedName: String?,
        mainID: UInt32?,
        actRealmWindowID: UInt32?,
        available: [HUDDisplayOption]
    ) -> UInt32? {
        switch mode {
        case .systemMain:
            return mainID ?? available.first?.id
        case .selectedDisplay:
            if let selectedID, available.contains(where: { $0.id == selectedID }) {
                return selectedID
            }
            if let selectedName,
               let reconnected = available.first(where: { $0.name == selectedName }) {
                return reconnected.id
            }
            return mainID ?? available.first?.id
        case .followActRealmWindow:
            if let actRealmWindowID,
               available.contains(where: { $0.id == actRealmWindowID }) {
                return actRealmWindowID
            }
            return mainID ?? available.first?.id
        }
    }
}

final class HUDInteractiveHostingView<Content: View>: NSHostingView<Content> {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override var mouseDownCanMoveWindow: Bool { false }
}

struct HUDDecisionSubmission: Equatable {
    let attentionID: String
    let action: AttentionAction
}

enum HUDDecisionInteraction {
    static func showsDecisionButtons(
        entryID: String,
        state: OutboxItemState,
        submission: HUDDecisionSubmission?
    ) -> Bool {
        state == .open && submission?.attentionID != entryID
    }
}

/// Top-level, non-activating approval capsule shown whenever something waits
/// on the user, including while the main window is in front.
@MainActor
public final class HUDPanelController {
    private let model: AppModel
    private var panel: NSPanel?
    private var cancellables: Set<AnyCancellable> = []

    public init(model: AppModel) {
        self.model = model
        model.$derived
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.updateVisibility() }
            .store(in: &cancellables)
        model.$isHUDPreviewActive
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.updateVisibility() }
            .store(in: &cancellables)
        model.$hudArrivalID
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.updateVisibility() }
            .store(in: &cancellables)
        model.$hudSettings
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.updateVisibility() }
            .store(in: &cancellables)
        model.$now
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.updateVisibility() }
            .store(in: &cancellables)
        model.$foregroundDispatch
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.updateVisibility() }
            .store(in: &cancellables)
        for name in [
            NSApplication.didBecomeActiveNotification,
            NSApplication.didResignActiveNotification,
            NSApplication.didChangeScreenParametersNotification,
        ] {
            NotificationCenter.default.publisher(for: name)
                .receive(on: RunLoop.main)
                .sink { [weak self] _ in self?.updateVisibility() }
                .store(in: &cancellables)
        }
        NotificationCenter.default.publisher(for: NSWindow.didChangeScreenNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] notification in
                guard let self,
                      let window = notification.object as? NSWindow,
                      self.isActRealmMainWindow(window)
                else { return }
                self.updateVisibility()
            }
            .store(in: &cancellables)
    }

    private var shouldShow: Bool {
        model.isHUDPreviewActive
            || model.foregroundDispatch != nil
            || (panel?.isVisible == true && undoablePendingDecision != nil)
            || (model.hudSettings.isEnabled
                && model.hudArrivalID != nil
                && (model.hudArrivalDeadline ?? .distantPast) > model.now)
    }

    private var undoablePendingDecision: PendingDecision? {
        guard let pending = model.derived.pendingDecision,
              pending.attentionID == model.hudArrivalID,
              case .undoable = pending.phase
        else { return nil }
        return pending
    }

    private func updateVisibility() {
        if shouldShow {
            presentPanel()
        } else {
            panel?.orderOut(nil)
        }
    }

    private func presentPanel() {
        let panel = ensurePanel()
        let wasVisible = panel.isVisible
        fitPanel(panel)
        positionAtTopCenter(panel)
        if !wasVisible {
            panel.orderFrontRegardless()
        }
    }

    private func ensurePanel() -> NSPanel {
        if let panel { return panel }
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 560, height: 76),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: true
        )
        panel.level = .statusBar
        panel.collectionBehavior = [
            .canJoinAllSpaces,
            .fullScreenAuxiliary,
            .stationary,
            .ignoresCycle,
        ]
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.isMovable = false
        panel.isMovableByWindowBackground = false
        panel.hidesOnDeactivate = false
        panel.becomesKeyOnlyIfNeeded = true
        panel.ignoresMouseEvents = false
        panel.isReleasedWhenClosed = false

        let host = HUDInteractiveHostingView(
            rootView: HUDCapsuleView()
            .environmentObject(model)
        )
        host.sizingOptions = [.intrinsicContentSize]
        panel.contentView = host
        self.panel = panel
        return panel
    }

    private func fitPanel(_ panel: NSPanel) {
        guard let contentView = panel.contentView else {
            panel.setContentSize(NSSize(width: 560, height: 76))
            return
        }
        contentView.layoutSubtreeIfNeeded()
        let measuredSize = contentView.fittingSize
        let currentSize = panel.contentLayoutRect.size
        let resolvedSize = NSSize(
            width: measuredSize.width.isFinite && measuredSize.width > 1
                ? measuredSize.width
                : max(currentSize.width, 560),
            height: measuredSize.height.isFinite && measuredSize.height > 1
                ? measuredSize.height
                : max(currentSize.height, 76)
        )
        panel.setContentSize(resolvedSize)
        contentView.frame = NSRect(origin: .zero, size: resolvedSize)
        contentView.layoutSubtreeIfNeeded()
    }

    private func positionAtTopCenter(_ panel: NSPanel) {
        guard let screen = targetScreen else { return }
        panel.setFrameOrigin(HUDPanelLayout.origin(
            screenFrame: screen.frame,
            visibleFrame: screen.visibleFrame,
            safeAreaInsets: screen.safeAreaInsets,
            panelSize: panel.frame.size
        ))
    }

    private var targetScreen: NSScreen? {
        let options = HUDDisplayCatalog.options
        let mainID = HUDDisplayCatalog.mainScreen.flatMap(HUDDisplayCatalog.displayID(for:))
        let windowID = actRealmMainWindow?.screen.flatMap(HUDDisplayCatalog.displayID(for:))
        let resolvedID = HUDDisplaySelection.resolve(
            mode: model.hudSettings.displayMode,
            selectedID: model.hudSettings.selectedDisplayID,
            selectedName: model.hudSettings.selectedDisplayName,
            mainID: mainID,
            actRealmWindowID: windowID,
            available: options
        )
        return resolvedID.flatMap(HUDDisplayCatalog.screen(with:))
            ?? HUDDisplayCatalog.mainScreen
    }

    private var actRealmMainWindow: NSWindow? {
        let candidates = NSApplication.shared.windows.filter(isActRealmMainWindow)
        return candidates.first {
            $0.identifier?.rawValue.hasPrefix("main-") == true
        } ?? candidates.first { $0.title.isEmpty }
    }

    private func isActRealmMainWindow(_ window: NSWindow) -> Bool {
        guard !(window is NSPanel), window.canBecomeMain else { return false }
        return window.identifier?.rawValue.hasPrefix("main-") == true
            || window.title.isEmpty
    }
}

// MARK: - Capsule content

public struct HUDCapsuleView: View {
    public init() {}

    @EnvironmentObject var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    @State private var decisionSubmission: HUDDecisionSubmission? = nil
    @State private var isHovering = false

    private var notification: OutboxEntry? {
        if let id = model.hudArrivalID,
           !model.isForegroundHUDSuppressed(for: id),
           let live = model.derived.openOutbox.first(where: { $0.id == id }) {
            return live
        }
        guard model.isHUDPreviewActive else { return nil }
        return DemoData.derivedState(now: model.now).openOutbox.first {
            $0.state == .open
        }
    }

    public var body: some View {
        VStack(spacing: 7) {
            if let pending = undoablePendingDecision {
                let content = undoContent(pending)
                    .padding(.horizontal, 15)
                    .padding(.vertical, 11)

                HUDCapsuleSurface(snapshotRendering: snapshotRendering) {
                    content
                }

                Text("3 秒内可以撤回 · 尚未写给 Provider")
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            } else if let dispatch = model.foregroundDispatch {
                let content = dispatchContent(dispatch)
                    .padding(.horizontal, 15)
                    .padding(.vertical, 11)

                HUDCapsuleSurface(snapshotRendering: snapshotRendering) {
                    content
                }

                Text(dispatchHint(dispatch))
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            } else if let entry = notification {
                let content = notificationContent(entry)
                    .padding(.horizontal, 15)
                    .padding(.vertical, 11)

                HUDCapsuleSurface(snapshotRendering: snapshotRendering) {
                    content
                }

                Text(entry.kind == .approval ? "可直接允许或拒绝 · 事项保留在待处理列表" : "事项已进入待处理列表")
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            }
        }
        .padding(14)
        .fixedSize()
        .overlay(alignment: .topLeading) {
            if isHovering && !snapshotRendering {
                Button {
                    isHovering = false
                    model.dismissHUD()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(DT.textSecondary)
                        .frame(width: 24, height: 24)
                        .background(DT.cardStrong.opacity(0.96), in: Circle())
                        .overlay(
                            Circle()
                                .strokeBorder(Color.white.opacity(0.36), lineWidth: 0.8)
                                .allowsHitTesting(false)
                        )
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .contentShape(Circle())
                .shadow(color: Color.black.opacity(0.28), radius: 6, y: 2)
                .padding(.leading, 3)
                .padding(.top, 3)
                .help("关闭通知")
                .accessibilityLabel("关闭通知")
                .transition(.opacity.combined(with: .scale(scale: 0.84)))
            }
        }
        .onHover { hovering in
            withAnimation(.easeOut(duration: 0.14)) {
                isHovering = hovering
            }
        }
    }

    private var undoablePendingDecision: PendingDecision? {
        guard let pending = model.derived.pendingDecision,
              pending.attentionID == (decisionSubmission?.attentionID ?? notification?.id),
              case .undoable = pending.phase
        else { return nil }
        return pending
    }

    // MARK: Approval

    @ViewBuilder
    private func dispatchContent(_ dispatch: ForegroundDispatchState) -> some View {
        switch dispatch.phase {
        case .reminding:
            HStack(spacing: 13) {
                dispatchRing(dispatch, color: DT.amberDot)
                VStack(alignment: .leading, spacing: 3) {
                    Text(dispatch.title)
                        .font(.system(size: 12.5, weight: .bold))
                        .foregroundStyle(DT.textStrong)
                        .lineLimit(1)
                    Text(dispatch.taskTitle.map { "任务：\($0)" } ?? "任务需要处理")
                        .font(DT.body(10.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(1)
                }
                .frame(maxWidth: 300, alignment: .leading)
                Button("立即查看") { model.openForegroundAgentNow() }
                    .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11, horizontalPadding: 14))
                Button("稍后处理") { model.keepForegroundTaskInActRealmWorkspace() }
                    .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11, horizontalPadding: 14))
            }
        case .opening:
            HStack(spacing: 12) {
                ProviderAvatar(kind: dispatch.provider ?? .codex, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text("正在聚焦 \(providerName(dispatch.provider))")
                        .font(.system(size: 12.5, weight: .bold))
                        .foregroundStyle(DT.textStrong)
                    Text(dispatch.title)
                        .font(DT.body(10.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(1)
                }
                .frame(maxWidth: 340, alignment: .leading)
            }
        case .awaitingWorkspace:
            HStack(spacing: 12) {
                dispatchRing(dispatch, color: DT.greenDot)
                VStack(alignment: .leading, spacing: 2) {
                    Text("等待鼠标进入绑定工作区 · \(remainingSeconds(dispatch)) 秒")
                        .font(.system(size: 12.5, weight: .bold))
                        .foregroundStyle(DT.textStrong)
                    Text("进入即视为已接收；事件仍等待批准、回答或确认")
                        .font(DT.body(10.5))
                        .foregroundStyle(DT.textWeak)
                }
                .frame(maxWidth: 360, alignment: .leading)
            }
        case .returnedToActRealmWorkspace:
            HStack(spacing: 10) {
                Image(systemName: "arrow.uturn.backward")
                    .font(.system(size: 13, weight: .bold))
                    .foregroundStyle(DT.amberText)
                    .frame(width: 32, height: 32)
                    .background(DT.amberBg, in: Circle())
                Text("未检测到接收，已返回 ActRealm")
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(DT.textStrong)
            }
        }
    }

    private func dispatchRing(_ dispatch: ForegroundDispatchState, color: Color) -> some View {
        ConicRing(
            fraction: dispatchFraction(dispatch),
            color: color,
            size: 42,
            lineWidth: 4,
            coreBackground: DT.cardStrong
        )
        .overlay(
            Text("\(remainingSeconds(dispatch))s")
                .font(.system(size: 9.5, weight: .heavy))
                .foregroundStyle(color)
        )
    }

    private func dispatchFraction(_ dispatch: ForegroundDispatchState) -> Double {
        let total = max(0.1, dispatch.deadline.timeIntervalSince(dispatch.startedAt))
        return max(0, dispatch.deadline.timeIntervalSince(model.now)) / total
    }

    private func remainingSeconds(_ dispatch: ForegroundDispatchState) -> Int {
        max(0, Int(dispatch.deadline.timeIntervalSince(model.now).rounded(.up)))
    }

    private func dispatchHint(_ dispatch: ForegroundDispatchState) -> String {
        switch dispatch.phase {
        case .reminding: "倒计时结束后自动聚焦 · 也可以稍后处理"
        case .opening: "优先打开具体任务；失败时打开 Agent 页面"
        case .awaitingWorkspace: "鼠标进入绑定工作区即视为已接收"
        case .returnedToActRealmWorkspace: "任务仍保留在待处理列表"
        }
    }

    private func providerName(_ provider: ProviderKind?) -> String {
        provider?.displayName ?? "Agent"
    }

    @ViewBuilder
    private func notificationContent(_ entry: OutboxEntry) -> some View {
        HStack(spacing: 13) {
            if model.hudSettings.fields.contains(.provider) {
                ProviderAvatar(kind: entry.provider ?? .codex, size: 38)
            } else if model.hudSettings.fields.contains(.elapsed), entry.kind == .approval {
                replyWindowRing(entry)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(headline(entry))
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(DT.textStrong)
                    .lineLimit(1)
                if !detailParts(entry).isEmpty {
                    Text(detailParts(entry).joined(separator: " · "))
                        .font(DT.body(10.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: 520, alignment: .leading)

            if entry.kind == .approval {
                if HUDDecisionInteraction.showsDecisionButtons(
                    entryID: entry.id,
                    state: entry.state,
                    submission: decisionSubmission
                ) {
                    Button {
                        submit(.deny, entry: entry)
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 13, weight: .bold))
                            .foregroundStyle(DT.redText)
                            .frame(width: 40, height: 40)
                            .background(Circle().fill(DT.redBg))
                            .overlay(Circle().strokeBorder(DT.redStroke, lineWidth: 1))
                            .contentShape(Circle())
                    }
                    .buttonStyle(.plain)
                    .contentShape(Circle())
                    .help("拒绝")

                    Button {
                        submit(.approve, entry: entry)
                    } label: {
                        Image(systemName: "checkmark")
                            .font(.system(size: 13, weight: .bold))
                            .foregroundStyle(.white)
                            .frame(width: 40, height: 40)
                            .background(Circle().fill(DT.primaryGradient))
                            .contentShape(Circle())
                    }
                    .buttonStyle(.plain)
                    .contentShape(Circle())
                    .shadow(color: DT.blue.opacity(0.4), radius: 8, y: 4)
                    .help("允许（3 秒内可撤回）")
                } else {
                    decisionSubmissionStatus(entry)
                }
            }
        }
    }

    private func submit(_ action: AttentionAction, entry: OutboxEntry) {
        guard HUDDecisionInteraction.showsDecisionButtons(
            entryID: entry.id,
            state: entry.state,
            submission: decisionSubmission
        ) else { return }

        decisionSubmission = HUDDecisionSubmission(attentionID: entry.id, action: action)
        switch action {
        case .approve: model.approve(entry)
        case .deny: model.deny(entry)
        default: return
        }

        Task { @MainActor in
            try? await Task.sleep(for: .seconds(2))
            guard decisionSubmission?.attentionID == entry.id,
                  model.derived.pendingDecision == nil,
                  model.derived.openOutbox.first(where: { $0.id == entry.id })?.state == .open
            else { return }
            decisionSubmission = nil
        }
    }

    @ViewBuilder
    private func decisionSubmissionStatus(_ entry: OutboxEntry) -> some View {
        HStack(spacing: 8) {
            if entry.state == .open {
                ProgressView()
                    .controlSize(.small)
            } else {
                Image(systemName: entry.state == .decisionSent ? "paperplane.fill" : "clock.fill")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(DT.greenText)
            }
            Text(decisionStatusText(entry))
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(DT.textWeak)
                .lineLimit(1)
        }
        .padding(.horizontal, 12)
        .frame(height: 40)
        .background(DT.greenBg.opacity(0.75), in: Capsule())
        .overlay(Capsule().strokeBorder(DT.greenStroke, lineWidth: 1))
    }

    private func decisionStatusText(_ entry: OutboxEntry) -> String {
        switch entry.state {
        case .open:
            return decisionSubmission?.action == .deny ? "正在提交拒绝…" : "正在提交允许…"
        case .committing:
            return "正在建立撤回窗口…"
        case .decisionSent:
            return "已写给 Provider，等待确认"
        case .snoozed, .resolved:
            return "请求状态已更新"
        }
    }

    private func headline(_ entry: OutboxEntry) -> String {
        if model.hudSettings.fields.contains(.event) { return entry.actionTitle }
        if model.hudSettings.fields.contains(.task), let task = entry.taskTitle { return task }
        return "ActRealm 通知"
    }

    private func detailParts(_ entry: OutboxEntry) -> [String] {
        var parts: [String] = []
        if model.hudSettings.fields.contains(.provider), entry.provider != nil {
            parts.append(providerName(entry))
        }
        if model.hudSettings.fields.contains(.task),
           let task = entry.taskTitle,
           task != headline(entry) {
            parts.append(task)
        }
        if model.hudSettings.fields.contains(.project),
           let project = entry.attention.project,
           !project.isEmpty {
            parts.append(project)
        }
        if model.hudSettings.fields.contains(.elapsed) {
            parts.append(ZhFormat.relativeAgo(model.now.timeIntervalSince(entry.createdAt)))
        }
        return parts
    }

    private func providerName(_ entry: OutboxEntry) -> String {
        entry.provider?.displayName ?? "Agent"
    }

    @ViewBuilder
    private func replyWindowRing(_ entry: OutboxEntry) -> some View {
        let remaining = entry.expiresAt.map { $0.timeIntervalSince(model.now) }
        let total = entry.expiresAt.map { $0.timeIntervalSince(entry.createdAt) }
        ConicRing(
            fraction: (remaining ?? 0) > 0 && (total ?? 0) > 0 ? remaining! / total! : 0,
            color: DT.amberDot,
            size: 42,
            lineWidth: 4,
            coreBackground: Color(light: .rgba(253, 249, 242, 1), dark: .rgba(20, 23, 34, 1))
        )
        .overlay(
            Text(remaining.map(clock) ?? "—")
                .font(DT.mono(9.5, weight: .semibold))
                .foregroundStyle(DT.amberText)
        )
    }

    private func clock(_ interval: TimeInterval) -> String {
        let total = max(0, Int(interval))
        if total >= 3600 { return "\(total / 3600)h\((total % 3600) / 60)m" }
        return String(format: "%02d:%02d", total / 60, total % 60)
    }

    // MARK: Undo

    @ViewBuilder
    private func undoContent(_ pending: PendingDecision) -> some View {
        let remaining: TimeInterval = {
            if case .undoable(let deadline) = pending.phase {
                return max(0, deadline.timeIntervalSince(model.now))
            }
            return 0
        }()
        HStack(spacing: 13) {
            ConicRing(
                fraction: remaining / DerivedState.undoWindow,
                color: DT.greenText,
                size: 42,
                lineWidth: 4,
                coreBackground: Color(light: .rgba(238, 247, 239, 1), dark: .rgba(24, 34, 26, 1))
            )
            .overlay(
                Text("\(Int(remaining.rounded(.up)))s")
                    .font(.system(size: 11, weight: .heavy))
                    .foregroundStyle(DT.greenText)
            )
            VStack(alignment: .leading, spacing: 2) {
                Text(pending.summary)
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(DT.textStrong)
                    .lineLimit(1)
                Text("倒计时结束后才会写给 Provider")
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            .frame(maxWidth: 260, alignment: .leading)
            Button("撤回") { model.undoPendingDecision() }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 12, horizontalPadding: 16))
        }
    }
}

private struct HUDCapsuleSurface<Content: View>: View {
    let snapshotRendering: Bool
    @ViewBuilder let content: Content

    var body: some View {
        ZStack {
            Capsule()
                .fill(DT.cardStrong.opacity(snapshotRendering ? 0.62 : 0.12))
                .shadow(
                    color: Color(red: 40 / 255, green: 60 / 255, blue: 120 / 255)
                        .opacity(0.28),
                    radius: 24,
                    y: 10
                )
                .allowsHitTesting(false)

            if snapshotRendering {
                content.background(DT.cardStrong.opacity(0.62), in: Capsule())
            } else {
                content.glassEffect(.regular, in: .capsule)
            }
        }
        .overlay(
            Capsule()
                .strokeBorder(Color.white.opacity(0.8), lineWidth: 1)
                .allowsHitTesting(false)
        )
    }
}
