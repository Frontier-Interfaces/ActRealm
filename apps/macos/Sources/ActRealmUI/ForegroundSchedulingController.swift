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

protocol StageManagerControlling {
    func isEnabled() -> Bool?
    @discardableResult func setEnabled(_ enabled: Bool) -> Bool
}

struct SystemStageManagerController: StageManagerControlling {
    typealias CommandResult = (status: Int32, output: String)
    typealias CommandRunner = (_ executable: String, _ arguments: [String]) -> CommandResult

    private let commandRunner: CommandRunner

    init(commandRunner: @escaping CommandRunner = Self.run) {
        self.commandRunner = commandRunner
    }

    func isEnabled() -> Bool? {
        let result = commandRunner("/usr/bin/defaults", [
            "read", "com.apple.WindowManager", "GloballyEnabled",
        ])
        guard result.status == 0 else { return nil }
        switch result.output.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "1", "true", "yes": return true
        case "0", "false", "no": return false
        default: return nil
        }
    }

    @discardableResult
    func setEnabled(_ enabled: Bool) -> Bool {
        let write = commandRunner("/usr/bin/defaults", [
            "write", "com.apple.WindowManager", "GloballyEnabled", "-bool",
            enabled ? "true" : "false",
        ])
        guard write.status == 0 else { return false }
        // WindowManager observes this preference on supported macOS versions.
        // Do not terminate or relaunch it: that causes a visible desktop flash
        // and can disturb the user's current windows and Spaces.
        return isEnabled() == enabled
    }

    private static func run(_ executable: String, _ arguments: [String]) -> CommandResult {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardOutput = pipe
        process.standardError = pipe
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return (-1, "")
        }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }
}

enum StageManagerRestoreTrigger: Equatable {
    case acceptance
    case actRealmReturn
    case focusDisabled
}

struct StageManagerLease: Equatable {
    private(set) var didEnableStageManager = false
    private(set) var restoreTiming: StageManagerRestoreTiming = .onReturnToActRealm

    mutating func begin(
        allowed: Bool,
        restoreTiming: StageManagerRestoreTiming,
        originalState: Bool?,
        enableSucceeded: Bool
    ) {
        guard !didEnableStageManager, allowed, originalState == false, enableSucceeded else { return }
        didEnableStageManager = true
        self.restoreTiming = restoreTiming
    }

    func shouldRestore(for trigger: StageManagerRestoreTrigger) -> Bool {
        guard didEnableStageManager else { return false }
        switch restoreTiming {
        case .afterAcceptance: return trigger == .acceptance || trigger == .focusDisabled
        case .onReturnToActRealm: return trigger == .actRealmReturn || trigger == .focusDisabled
        case .keepEnabled: return false
        }
    }

    mutating func finishRestore(succeeded: Bool) {
        if succeeded { didEnableStageManager = false }
    }
}

/// Executes the AppKit half of Agent Focus: foregrounding a matching Agent,
/// observing the bound workspace and pointer, and restoring any Stage Manager
/// state that this focus session changed. ActRealmKit owns policy and timers.
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
    private var dispatchObservationTask: Task<Void, Never>?
    private var openingTask: Task<Void, Never>?
    private let stageManagerController: any StageManagerControlling
    private var stageManagerLease = StageManagerLease()

    public convenience init(model: AppModel) {
        self.init(model: model, stageManagerController: SystemStageManagerController())
    }

    init(model: AppModel, stageManagerController: any StageManagerControlling) {
        self.model = model
        self.stageManagerController = stageManagerController

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
                self?.acceptIfPointerEnteredBoundWorkspace()
                if self?.model.isSelectingForegroundWorkspace == true {
                    self?.refreshVisibleWorkspaceApps()
                }
            }
            .store(in: &cancellables)

        NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.didActivateApplicationNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] notification in
                guard let self,
                      let application = notification.userInfo?[NSWorkspace.applicationUserInfoKey]
                        as? NSRunningApplication,
                      application.bundleIdentifier == Bundle.main.bundleIdentifier
                else { return }
                self.restoreStageManagerIfNeeded(for: .actRealmReturn)
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
        dispatchObservationTask?.cancel()
        openingTask?.cancel()
    }

    private func handle(_ dispatch: ForegroundDispatchState?) {
        guard let dispatch else {
            dispatchObservationTask?.cancel()
            openingTask?.cancel()
            if !model.foregroundScheduling.isEnabled {
                restoreStageManagerIfNeeded(for: .focusDisabled)
            }
            if stageManagerLease.restoreTiming == .keepEnabled {
                stageManagerLease.finishRestore(succeeded: true)
            }
            lastHandledID = nil
            lastHandledPhase = nil
            return
        }
        guard dispatch.id != lastHandledID || dispatch.phase != lastHandledPhase else { return }
        if dispatch.id != lastHandledID {
            startDispatchObservation()
        }
        lastHandledID = dispatch.id
        lastHandledPhase = dispatch.phase

        refreshWorkspaceStatus()
        if acceptsCurrentFocus(dispatch) { return }

        switch dispatch.phase {
        case .reminding:
            break
        case .opening:
            beginOpening(dispatch)
        case .awaitingWorkspace:
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(180))
                self?.refreshWorkspaceStatus()
                self?.acceptIfPointerEnteredBoundWorkspace()
            }
        case .returnedToActRealmWorkspace:
            activateActRealmWorkspace()
            restoreStageManagerIfNeeded(for: .actRealmReturn)
        }
    }

    private func beginOpening(_ dispatch: ForegroundDispatchState) {
        prepareStageManagerIfNeeded()
        openingTask?.cancel()
        openingTask = Task { @MainActor [weak self] in
            guard let self else { return }
            let openedSpecificTask = await self.model.focusSpecificTask(attentionID: dispatch.id)
            guard !Task.isCancelled,
                  self.model.foregroundDispatch?.id == dispatch.id,
                  self.model.foregroundDispatch?.phase == .opening
            else { return }
            let activatedAgent = self.activateAgent(for: dispatch.provider, reportFailure: false)
            if !openedSpecificTask && !activatedAgent {
                self.model.failForegroundTargetActivation()
            }
        }
    }

    @discardableResult
    private func activateAgent(for provider: ProviderKind?, reportFailure: Bool = true) -> Bool {
        guard let target = agentTarget(for: provider) else {
            if reportFailure {
                model.showToast("未找到对应 Agent 窗口；事件仍保留在 ActRealm")
            }
            refreshWorkspaceStatus()
            return false
        }
        _ = target.activate(options: [])
        refreshWorkspaceStatus()
        return true
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

    private func acceptsCurrentFocus(_ dispatch: ForegroundDispatchState) -> Bool {
        guard model.foregroundWorkspaceStatus.isPointerInsideSchedulingWorkspace else {
            return false
        }
        switch dispatch.phase {
        case .reminding, .opening:
            restoreStageManagerIfNeeded(for: .acceptance)
            model.acceptForegroundInPlace()
            return true
        case .awaitingWorkspace:
            restoreStageManagerIfNeeded(for: .acceptance)
            model.acceptForegroundWorkspace()
            return true
        case .returnedToActRealmWorkspace:
            return false
        }
    }

    private func acceptIfPointerEnteredBoundWorkspace() {
        guard let dispatch = model.foregroundDispatch else { return }
        _ = acceptsCurrentFocus(dispatch)
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
        let provider = model.foregroundDispatch?.provider
        let workspaceActive = isBoundWorkspaceActive(for: provider)
        model.updateForegroundWorkspaceStatus(ForegroundWorkspaceStatus(
            isActRealmWorkspaceReady: window != nil,
            isAgentAvailable: agentTarget(for: provider) != nil,
            isSchedulingWorkspaceActive: workspaceActive,
            isPointerInsideSchedulingWorkspace: workspaceActive && isPointerInsideBoundDisplay
        ))
    }

    private func isBoundWorkspaceActive(for provider: ProviderKind?) -> Bool {
        let bound = Set(model.foregroundScheduling.workspaceApps.filter {
            workspaceApplication($0, matches: provider)
        }.map(\.bundleIdentifier))
        guard !bound.isEmpty else { return false }
        let visible = Set(visibleApplications(
            on: resolvedBoundDisplayID
        ).map(\.bundleIdentifier))
        return !bound.isDisjoint(with: visible)
    }

    private var isPointerInsideBoundDisplay: Bool {
        guard let displayID = resolvedBoundDisplayID,
              let screen = screen(with: displayID)
        else { return false }
        return screen.frame.contains(NSEvent.mouseLocation)
    }

    private func workspaceApplication(
        _ application: ForegroundWorkspaceApp,
        matches provider: ProviderKind?
    ) -> Bool {
        guard let provider else { return true }
        let identity = "\(application.bundleIdentifier) \(application.name)".lowercased()
        let providerTokens: [String]
        switch provider {
        case .codex: providerTokens = ["codex", "chatgpt"]
        case .claude: providerTokens = ["claude"]
        case .gemini: providerTokens = ["gemini"]
        case let .custom(value): providerTokens = [value.lowercased()]
        }
        return providerTokens.contains(where: identity.contains)
            || ["terminal", "iterm", "warp", "wezterm", "alacritty"].contains(
                where: identity.contains
            )
    }

    private func startDispatchObservation() {
        dispatchObservationTask?.cancel()
        dispatchObservationTask = Task { @MainActor [weak self] in
            while !Task.isCancelled, self?.model.foregroundDispatch != nil {
                self?.refreshWorkspaceStatus()
                self?.acceptIfPointerEnteredBoundWorkspace()
                try? await Task.sleep(for: .milliseconds(200))
            }
        }
    }

    private func prepareStageManagerIfNeeded() {
        guard !stageManagerLease.didEnableStageManager,
              model.foregroundScheduling.allowsStageManager
        else { return }
        let originalState = stageManagerController.isEnabled()
        let enabled = originalState == false && stageManagerController.setEnabled(true)
        stageManagerLease.begin(
            allowed: true,
            restoreTiming: model.foregroundScheduling.stageManagerRestoreTiming,
            originalState: originalState,
            enableSucceeded: enabled
        )
        if originalState == nil || (originalState == false && !enabled) {
            model.showToast("无法更改 macOS 台前调度；仍继续聚焦 Agent")
        }
    }

    private func restoreStageManagerIfNeeded(for trigger: StageManagerRestoreTrigger) {
        guard stageManagerLease.shouldRestore(for: trigger) else { return }
        let restored = stageManagerController.setEnabled(false)
        stageManagerLease.finishRestore(succeeded: restored)
        if !restored {
            model.showToast("无法恢复台前调度进入前状态，请在控制中心检查")
        }
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
        panel.title = "选择 Agent 绑定工作区"
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
                Text("选择 Agent 绑定工作区")
                    .font(.title2.weight(.bold))
                Text("切换到放置 Claude、Codex 或终端的工作区，再绑定当前显示器上的窗口。")
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

            GroupBox("所选显示器当前工作区的应用") {
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

            Text("智能聚焦只会处理已绑定的 Agent；鼠标进入该工作区即视为已接收。")
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
