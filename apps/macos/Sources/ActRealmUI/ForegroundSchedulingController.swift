import AppKit
import ActRealmKit
import Combine
import SwiftUI

public struct WorkspaceDisplayOption: Identifiable, Equatable, Sendable {
    public let id: UInt32
    public let name: String
    public let isPrimary: Bool

    public init(id: UInt32, name: String, isPrimary: Bool) {
        self.id = id
        self.name = name
        self.isPrimary = isPrimary
    }

    public var label: String { isPrimary ? "\(name)（主显示器）" : name }
}

/// Executes the AppKit half of 台前调度: foregrounding a matching Agent app,
/// observing active Space changes, and returning the ActRealm Workspace window after an
/// unaccepted dispatch. ActRealmKit remains responsible for policy/timers.
@MainActor
public final class ForegroundSchedulingController: ObservableObject {
    @Published private(set) var visibleWorkspaceApps: [ForegroundWorkspaceApp] = []
    @Published private(set) var availableWorkspaceDisplays: [WorkspaceDisplayOption] = []
    @Published private(set) var selectedWorkspaceDisplayID: UInt32?

    private let model: AppModel
    private var cancellables: Set<AnyCancellable> = []
    private var lastHandledPhase: ForegroundDispatchPhase?
    private var lastHandledID: String?
    private var workspaceSelectionPanel: NSPanel?
    private var selectionRefreshTask: Task<Void, Never>?

    public init(model: AppModel) {
        self.model = model

        model.$foregroundDispatch
            .receive(on: RunLoop.main)
            .sink { [weak self] dispatch in
                self?.handle(dispatch)
            }
            .store(in: &cancellables)

        model.$isSelectingForegroundWorkspace
            .receive(on: RunLoop.main)
            .sink { [weak self] selecting in
                selecting ? self?.presentWorkspaceSelection() : self?.dismissWorkspaceSelection()
            }
            .store(in: &cancellables)

        NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.activeSpaceDidChangeNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                self?.refreshWorkspaceStatus()
                self?.acceptIfSchedulingWorkspaceIsActive()
                if self?.model.isSelectingForegroundWorkspace == true {
                    self?.refreshVisibleWorkspaceApps()
                }
            }
            .store(in: &cancellables)

        for name in [NSWorkspace.didLaunchApplicationNotification, NSWorkspace.didTerminateApplicationNotification] {
            NSWorkspace.shared.notificationCenter.publisher(for: name)
                .receive(on: RunLoop.main)
                .sink { [weak self] _ in self?.refreshWorkspaceStatus() }
                .store(in: &cancellables)
        }

        NotificationCenter.default.publisher(for: NSApplication.didChangeScreenParametersNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                self?.refreshWorkspaceDisplays()
                self?.refreshVisibleWorkspaceApps()
            }
            .store(in: &cancellables)

        Task { @MainActor [weak self] in
            await Task.yield()
            self?.refreshWorkspaceDisplays()
            self?.refreshWorkspaceStatus()
        }
    }

    deinit {
        selectionRefreshTask?.cancel()
    }

    private func handle(_ dispatch: ForegroundDispatchState?) {
        guard let dispatch else {
            lastHandledID = nil
            lastHandledPhase = nil
            return
        }
        guard dispatch.id != lastHandledID || dispatch.phase != lastHandledPhase else { return }
        lastHandledID = dispatch.id
        lastHandledPhase = dispatch.phase

        switch dispatch.phase {
        case .reminding:
            break
        case .opening:
            activateAgent(for: dispatch.provider)
        case .awaitingWorkspace:
            // App activation and Mission Control notifications settle on the
            // following run-loop turn. If the Agent was already beside Act
            // Room, this immediately satisfies the documented receive rule.
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(180))
                self?.refreshWorkspaceStatus()
                self?.acceptIfSchedulingWorkspaceIsActive()
            }
        case .returnedToActRealmWorkspace:
            activateActRealmWorkspace()
        }
    }

    private func activateAgent(for provider: ProviderKind?) {
        guard let target = agentTarget(for: provider) else {
            model.showToast("未找到对应 Agent 窗口；任务仍保留在 ActRealm 工作区")
            refreshWorkspaceStatus()
            return
        }
        _ = target.activate(options: [])
        refreshWorkspaceStatus()
    }

    private func agentTarget(for provider: ProviderKind?) -> NSRunningApplication? {
        let running = NSWorkspace.shared.runningApplications.filter { !$0.isTerminated }
        let providerTokens: [String]
        switch provider {
        case .codex: providerTokens = ["codex"]
        case .claude: providerTokens = ["claude"]
        case .gemini: providerTokens = ["gemini"]
        case let .custom(value): providerTokens = [value]
        case nil: providerTokens = ["codex", "claude", "gemini", "cursor"]
        }

        return running.first(where: { app in
            let identity = "\(app.bundleIdentifier ?? "") \(app.localizedName ?? "")".lowercased()
            return providerTokens.contains { identity.contains($0) }
        }) ?? running.first(where: { app in
            let identity = "\(app.bundleIdentifier ?? "") \(app.localizedName ?? "")".lowercased()
            return ["terminal", "iterm", "warp", "wezterm", "alacritty"].contains {
                identity.contains($0)
            }
        })
    }

    private func acceptIfSchedulingWorkspaceIsActive() {
        guard model.foregroundDispatch?.phase == .awaitingWorkspace,
              isBoundWorkspaceActive
        else { return }
        model.acceptForegroundWorkspace()
    }

    private func activateActRealmWorkspace() {
        NSApplication.shared.activate(ignoringOtherApps: true)
        actRealmWorkspaceWindow?.makeKeyAndOrderFront(nil)
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(120))
            self?.refreshWorkspaceStatus()
        }
    }

    private func refreshWorkspaceStatus() {
        let window = actRealmWorkspaceWindow
        model.updateForegroundWorkspaceStatus(ForegroundWorkspaceStatus(
            isActRealmWorkspaceReady: window != nil,
            isAgentAvailable: agentTarget(for: model.foregroundDispatch?.provider) != nil,
            isSchedulingWorkspaceActive: isBoundWorkspaceActive
        ))
    }

    private var isBoundWorkspaceActive: Bool {
        let bound = Set(model.foregroundScheduling.workspaceApps.map(\.bundleIdentifier))
        guard !bound.isEmpty else { return false }
        let visible = Set(visibleApplications(
            on: resolvedBoundDisplayID
        ).map(\.bundleIdentifier))
        let requiredMatches = max(1, Int(ceil(Double(bound.count) * 0.5)))
        return bound.intersection(visible).count >= requiredMatches
    }

    private func presentWorkspaceSelection() {
        refreshWorkspaceDisplays()
        refreshVisibleWorkspaceApps()
        let panel = ensureWorkspaceSelectionPanel()
        positionWorkspaceSelectionPanel(panel)
        NSApplication.shared.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
        selectionRefreshTask?.cancel()
        selectionRefreshTask = Task { @MainActor [weak self] in
            while !Task.isCancelled, self?.model.isSelectingForegroundWorkspace == true {
                try? await Task.sleep(for: .seconds(1))
                self?.refreshVisibleWorkspaceApps()
            }
        }
    }

    private func dismissWorkspaceSelection() {
        selectionRefreshTask?.cancel()
        workspaceSelectionPanel?.orderOut(nil)
    }

    private func ensureWorkspaceSelectionPanel() -> NSPanel {
        if let workspaceSelectionPanel { return workspaceSelectionPanel }
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 540, height: 330),
            styleMask: [.titled],
            backing: .buffered,
            defer: true
        )
        panel.title = "选择协作桌面"
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.hidesOnDeactivate = false
        panel.isReleasedWhenClosed = false
        panel.contentView = NSHostingView(rootView: WorkspaceSelectionPanelView(
            controller: self,
            onConfirm: { [weak self] in self?.confirmWorkspaceSelection() },
            onCancel: { [weak self] in self?.model.cancelForegroundWorkspaceSelection() }
        ))
        workspaceSelectionPanel = panel
        return panel
    }

    private func positionWorkspaceSelectionPanel(_ panel: NSPanel) {
        guard let screen = selectedWorkspaceDisplayID.flatMap(screen(with:))
            ?? NSScreen.main
        else { return }
        let frame = screen.visibleFrame
        panel.setFrameOrigin(NSPoint(
            x: frame.midX - panel.frame.width / 2,
            y: frame.midY - panel.frame.height / 2
        ))
    }

    fileprivate func refreshVisibleWorkspaceApps() {
        visibleWorkspaceApps = visibleApplications(on: selectedWorkspaceDisplayID)
        refreshWorkspaceStatus()
    }

    fileprivate func selectWorkspaceDisplay(_ displayID: UInt32) {
        guard availableWorkspaceDisplays.contains(where: { $0.id == displayID }) else { return }
        selectedWorkspaceDisplayID = displayID
        refreshVisibleWorkspaceApps()
        if let panel = workspaceSelectionPanel, panel.isVisible {
            positionWorkspaceSelectionPanel(panel)
        }
    }

    private func confirmWorkspaceSelection() {
        refreshVisibleWorkspaceApps()
        let display = availableWorkspaceDisplays.first { $0.id == selectedWorkspaceDisplayID }
        model.bindForegroundWorkspace(
            apps: visibleWorkspaceApps,
            displayID: display?.id,
            displayName: display?.name
        )
        refreshWorkspaceStatus()
    }

    private func refreshWorkspaceDisplays() {
        availableWorkspaceDisplays = NSScreen.screens.compactMap { screen in
            guard let id = displayID(for: screen) else { return nil }
            return WorkspaceDisplayOption(
                id: id,
                name: screen.localizedName,
                isPrimary: screen.frame.origin == .zero
            )
        }

        let availableIDs = Set(availableWorkspaceDisplays.map(\.id))
        if let selectedWorkspaceDisplayID, availableIDs.contains(selectedWorkspaceDisplayID) {
            return
        }
        if let persisted = model.foregroundScheduling.workspaceDisplayID,
           availableIDs.contains(persisted)
        {
            selectedWorkspaceDisplayID = persisted
            return
        }
        if let persistedName = model.foregroundScheduling.workspaceDisplayName,
           let matchingDisplay = availableWorkspaceDisplays.first(where: {
               $0.name == persistedName
           })
        {
            selectedWorkspaceDisplayID = matchingDisplay.id
            return
        }
        let pointer = NSEvent.mouseLocation
        selectedWorkspaceDisplayID = NSScreen.screens
            .first(where: { $0.frame.contains(pointer) })
            .flatMap(displayID(for:))
            ?? NSScreen.main.flatMap(displayID(for:))
            ?? availableWorkspaceDisplays.first?.id
    }

    private func visibleApplications(on displayID: UInt32?) -> [ForegroundWorkspaceApp] {
        guard let windows = CGWindowListCopyWindowInfo(
            [.optionOnScreenOnly, .excludeDesktopElements],
            kCGNullWindowID
        ) as? [[String: Any]] else { return [] }

        let excluded = Set([Bundle.main.bundleIdentifier, "com.apple.finder", "com.apple.dock"].compactMap { $0 })
        var applications: [String: ForegroundWorkspaceApp] = [:]
        for window in windows {
            guard (window[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
                  isWindow(window, visibleOn: displayID),
                  let pidValue = window[kCGWindowOwnerPID as String] as? NSNumber,
                  let application = NSRunningApplication(processIdentifier: pid_t(pidValue.intValue)),
                  application.activationPolicy == .regular,
                  let identifier = application.bundleIdentifier,
                  !excluded.contains(identifier)
            else { continue }
            let name = application.localizedName
                ?? (window[kCGWindowOwnerName as String] as? String)
                ?? identifier
            applications[identifier] = ForegroundWorkspaceApp(
                bundleIdentifier: identifier,
                name: name
            )
        }
        return applications.values.sorted {
            $0.name.localizedStandardCompare($1.name) == .orderedAscending
        }
    }

    private func isWindow(_ window: [String: Any], visibleOn displayID: UInt32?) -> Bool {
        guard let displayID else { return true }
        guard let boundsDictionary = window[kCGWindowBounds as String] as? NSDictionary,
              let windowBounds = CGRect(
                dictionaryRepresentation: boundsDictionary as CFDictionary
              )
        else { return false }
        let intersection = windowBounds.intersection(CGDisplayBounds(displayID))
        return !intersection.isNull && intersection.width > 1 && intersection.height > 1
    }

    private func displayID(for screen: NSScreen) -> UInt32? {
        (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?
            .uint32Value
    }

    private func screen(with displayID: UInt32) -> NSScreen? {
        NSScreen.screens.first { self.displayID(for: $0) == displayID }
    }

    private var resolvedBoundDisplayID: UInt32? {
        if let storedID = model.foregroundScheduling.workspaceDisplayID,
           availableWorkspaceDisplays.contains(where: { $0.id == storedID })
        {
            return storedID
        }
        guard let storedName = model.foregroundScheduling.workspaceDisplayName else {
            return model.foregroundScheduling.workspaceDisplayID
        }
        return availableWorkspaceDisplays.first(where: { $0.name == storedName })?.id
            ?? model.foregroundScheduling.workspaceDisplayID
    }

    private var actRealmWorkspaceWindow: NSWindow? {
        NSApplication.shared.windows.first {
            !($0 is NSPanel) && $0.isVisible && $0.canBecomeMain
        }
    }
}

private struct WorkspaceSelectionPanelView: View {
    @ObservedObject var controller: ForegroundSchedulingController
    let onConfirm: () -> Void
    let onCancel: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 5) {
                Text("选择协作桌面")
                    .font(.title2.weight(.bold))
                Text("切换到放置 Claude、Codex、浏览器等协作应用的虚拟桌面，再绑定当前桌面。")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            if controller.availableWorkspaceDisplays.count > 1,
               let selectedDisplayID = controller.selectedWorkspaceDisplayID
            {
                Picker("识别显示器", selection: Binding(
                    get: { selectedDisplayID },
                    set: { controller.selectWorkspaceDisplay($0) }
                )) {
                    ForEach(controller.availableWorkspaceDisplays) { display in
                        Text(display.label).tag(display.id)
                    }
                }
                .pickerStyle(.menu)
            }

            GroupBox("所选显示器当前桌面的应用") {
                if controller.visibleWorkspaceApps.isEmpty {
                    Text("未检测到可绑定的应用窗口")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    ScrollView(.horizontal) {
                        HStack(spacing: 8) {
                            ForEach(controller.visibleWorkspaceApps) { application in
                                Text(application.name)
                                    .font(.callout.weight(.medium))
                                    .padding(.horizontal, 9)
                                    .padding(.vertical, 4)
                                    .background(.quaternary, in: Capsule())
                            }
                        }
                    }
                    .scrollIndicators(.hidden)
                }
            }

            Text("可选择内建或外接显示器。切换该显示器的虚拟桌面后，点击“重新检测”再绑定。")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack {
                Button("取消", action: onCancel)
                Button("重新检测") { controller.refreshVisibleWorkspaceApps() }
                Spacer()
                Button("绑定当前桌面", action: onConfirm)
                    .buttonStyle(.borderedProminent)
                    .disabled(controller.visibleWorkspaceApps.isEmpty)
            }
        }
        .padding(22)
        .frame(width: 540, height: 330)
    }
}
