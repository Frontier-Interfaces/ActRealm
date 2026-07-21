import AppKit
import Combine
import ActRealmKit
import SwiftUI

/// Floating, non-activating approval capsule shown whenever something waits
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
        for name in [NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification] {
            NotificationCenter.default.publisher(for: name)
                .receive(on: RunLoop.main)
                .sink { [weak self] _ in self?.updateVisibility() }
                .store(in: &cancellables)
        }
    }

    private var shouldShow: Bool {
        model.isHUDPreviewActive
            || model.foregroundDispatch != nil
            || (model.hudSettings.isEnabled
                && model.hudArrivalID != nil
                && (model.hudArrivalDeadline ?? .distantPast) > model.now)
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
        panel.setContentSize(panel.contentView?.fittingSize ?? NSSize(width: 560, height: 76))
        position(panel)
        if !panel.isVisible {
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
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.isMovableByWindowBackground = true
        panel.hidesOnDeactivate = false
        panel.becomesKeyOnlyIfNeeded = true

        let host = NSHostingView(rootView: HUDCapsuleView().environmentObject(model))
        host.sizingOptions = [.preferredContentSize]
        panel.contentView = host
        self.panel = panel
        return panel
    }

    private func position(_ panel: NSPanel) {
        guard let screen = NSScreen.main else { return }
        panel.setContentSize(panel.contentView?.fittingSize ?? NSSize(width: 560, height: 76))
        let frame = screen.visibleFrame
        let size = panel.frame.size
        let origin = NSPoint(
            x: frame.midX - size.width / 2,
            y: frame.maxY - size.height - 14
        )
        panel.setFrameOrigin(origin)
    }
}

// MARK: - Capsule content

public struct HUDCapsuleView: View {
    public init() {}

    @EnvironmentObject var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering

    private var notification: OutboxEntry? {
        if let id = model.hudArrivalID,
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
            if let dispatch = model.foregroundDispatch {
                let content = dispatchContent(dispatch)
                    .padding(.horizontal, 15)
                    .padding(.vertical, 11)

                Group {
                    if snapshotRendering {
                        content.background(DT.cardStrong.opacity(0.62), in: Capsule())
                    } else {
                        content.glassEffect(.regular, in: .capsule)
                    }
                }
                .overlay(Capsule().strokeBorder(Color.white.opacity(0.8), lineWidth: 1))
                .compositingGroup()
                .shadow(color: Color(red: 40 / 255, green: 60 / 255, blue: 120 / 255).opacity(0.30), radius: 30, y: 12)

                Text(dispatchHint(dispatch))
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            } else if let entry = notification {
                let content = notificationContent(entry)
                    .padding(.horizontal, 15)
                    .padding(.vertical, 11)

                Group {
                    if snapshotRendering {
                        content.background(DT.cardStrong.opacity(0.62), in: Capsule())
                    } else {
                        content.glassEffect(.regular, in: .capsule)
                    }
                }
                .overlay(Capsule().strokeBorder(Color.white.opacity(0.8), lineWidth: 1))
                .compositingGroup()
                .shadow(color: Color(red: 40 / 255, green: 60 / 255, blue: 120 / 255).opacity(0.30), radius: 30, y: 12)

                Text(entry.kind == .approval ? "可直接允许或拒绝 · 事项保留在待处理列表" : "事项已进入待处理列表")
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            }
        }
        .padding(14)
        .fixedSize()
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
                Button("立即打开") { model.openForegroundAgentNow() }
                    .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11, horizontalPadding: 14))
                Button("留在 ActRealm 工作区") { model.keepForegroundTaskInActRealmWorkspace() }
                    .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11, horizontalPadding: 14))
            }
        case .opening:
            HStack(spacing: 12) {
                ProviderAvatar(kind: dispatch.provider ?? .codex, size: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text("正在打开 \(providerName(dispatch.provider))")
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
                    Text("等待进入协作桌面 · \(remainingSeconds(dispatch)) 秒")
                        .font(.system(size: 12.5, weight: .bold))
                        .foregroundStyle(DT.textStrong)
                    Text("协作应用出现在当前桌面后，自动返回倒计时会停止")
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
                Text("未检测到进入调度工作桌面，已返回 ActRealm 工作区")
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
        case .reminding: "倒计时结束后自动打开 · 也可以留在 ActRealm 工作区"
        case .opening: "正在把对应 Agent 带到前台"
        case .awaitingWorkspace: "进入调度工作桌面即视为已接收"
        case .returnedToActRealmWorkspace: "任务仍保留在待处理列表"
        }
    }

    private func providerName(_ provider: ProviderKind?) -> String {
        switch provider {
        case .codex: "Codex"
        case .claude: "Claude"
        case .gemini: "Gemini"
        case nil: "Agent"
        }
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
                Button {
                    model.deny(entry)
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .bold))
                        .foregroundStyle(DT.redText)
                        .frame(width: 36, height: 36)
                        .background(Circle().fill(DT.redBg))
                        .overlay(Circle().strokeBorder(DT.redStroke, lineWidth: 1))
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .help("拒绝")

                Button {
                    model.approve(entry)
                } label: {
                    Image(systemName: "checkmark")
                        .font(.system(size: 13, weight: .bold))
                        .foregroundStyle(.white)
                        .frame(width: 36, height: 36)
                        .background(Circle().fill(DT.primaryGradient))
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .shadow(color: DT.blue.opacity(0.4), radius: 8, y: 4)
                .help("允许（3 秒内可撤回）")
            }
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
        switch entry.provider {
        case .codex: "Codex"
        case .claude: "Claude"
        case .gemini: "Gemini"
        case nil: "Agent"
        }
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
                Text("决定已发送，等待确认")
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            .frame(maxWidth: 260, alignment: .leading)
            Button("撤回") { model.undoPendingDecision() }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 12, horizontalPadding: 16))
        }
    }
}
