import Combine
import Foundation

public enum BridgeStatus: Equatable, Sendable {
    case listening
    case starting
    case absent(String?)

    public var isListening: Bool { self == .listening }
}

/// Mutually-exclusive policy used when a task reaches the front-of-house
/// attention queue.
public enum ForegroundArrivalStrategy: String, CaseIterable, Codable, Sendable, Hashable {
    case immediate
    case remind
    case actRealmWorkspace
}

/// Optional per-provider override. `defaultRule` follows the global arrival
/// strategy while the remaining cases pin that provider to a concrete policy.
public enum ForegroundAgentRule: String, CaseIterable, Codable, Sendable, Hashable {
    case defaultRule
    case immediate
    case remind
    case actRealmWorkspace
}

public struct ForegroundWorkspaceApp: Codable, Equatable, Sendable, Identifiable {
    public let bundleIdentifier: String
    public let name: String

    public var id: String { bundleIdentifier }

    public init(bundleIdentifier: String, name: String) {
        self.bundleIdentifier = bundleIdentifier
        self.name = name
    }
}

/// Durable settings edited by the 台前调度 management page. Keeping the
/// policy in ActRealmKit gives the Runtime integration one authoritative
/// value instead of leaving behavior hidden inside view-local state.
public struct ForegroundSchedulingSettings: Codable, Equatable, Sendable {
    public var isEnabled: Bool
    public var closesAfterAcceptance: Bool
    public var strategy: ForegroundArrivalStrategy
    public var reminderSeconds: Int
    public var returnsToActRealmWorkspace: Bool
    public var acceptanceSeconds: Int
    public var codexRule: ForegroundAgentRule
    public var claudeRule: ForegroundAgentRule
    public var workspaceApps: [ForegroundWorkspaceApp]
    public var workspaceBoundAt: Date?
    public var workspaceDisplayID: UInt32?
    public var workspaceDisplayName: String?

    public init(
        isEnabled: Bool = true,
        closesAfterAcceptance: Bool = true,
        strategy: ForegroundArrivalStrategy = .remind,
        reminderSeconds: Int = 10,
        returnsToActRealmWorkspace: Bool = true,
        acceptanceSeconds: Int = 10,
        codexRule: ForegroundAgentRule = .defaultRule,
        claudeRule: ForegroundAgentRule = .immediate,
        workspaceApps: [ForegroundWorkspaceApp] = [],
        workspaceBoundAt: Date? = nil,
        workspaceDisplayID: UInt32? = nil,
        workspaceDisplayName: String? = nil
    ) {
        self.isEnabled = isEnabled
        self.closesAfterAcceptance = closesAfterAcceptance
        self.strategy = strategy
        self.reminderSeconds = reminderSeconds
        self.returnsToActRealmWorkspace = returnsToActRealmWorkspace
        self.acceptanceSeconds = acceptanceSeconds
        self.codexRule = codexRule
        self.claudeRule = claudeRule
        self.workspaceApps = workspaceApps
        self.workspaceBoundAt = workspaceBoundAt
        self.workspaceDisplayID = workspaceDisplayID
        self.workspaceDisplayName = workspaceDisplayName
    }

    public static let defaults = ForegroundSchedulingSettings()

    private enum CodingKeys: String, CodingKey {
        case isEnabled, closesAfterAcceptance, strategy, reminderSeconds
        case returnsToActRealmWorkspace, acceptanceSeconds, codexRule, claudeRule
        case workspaceApps, workspaceBoundAt, workspaceDisplayID, workspaceDisplayName
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        isEnabled = try values.decodeIfPresent(Bool.self, forKey: .isEnabled) ?? true
        closesAfterAcceptance = try values.decodeIfPresent(Bool.self, forKey: .closesAfterAcceptance) ?? true
        strategy = try values.decodeIfPresent(ForegroundArrivalStrategy.self, forKey: .strategy) ?? .remind
        reminderSeconds = try values.decodeIfPresent(Int.self, forKey: .reminderSeconds) ?? 10
        returnsToActRealmWorkspace = try values.decodeIfPresent(Bool.self, forKey: .returnsToActRealmWorkspace) ?? true
        acceptanceSeconds = try values.decodeIfPresent(Int.self, forKey: .acceptanceSeconds) ?? 10
        codexRule = try values.decodeIfPresent(ForegroundAgentRule.self, forKey: .codexRule) ?? .defaultRule
        claudeRule = try values.decodeIfPresent(ForegroundAgentRule.self, forKey: .claudeRule) ?? .immediate
        workspaceApps = try values.decodeIfPresent([ForegroundWorkspaceApp].self, forKey: .workspaceApps) ?? []
        workspaceBoundAt = try values.decodeIfPresent(Date.self, forKey: .workspaceBoundAt)
        workspaceDisplayID = try values.decodeIfPresent(UInt32.self, forKey: .workspaceDisplayID)
        workspaceDisplayName = try values.decodeIfPresent(String.self, forKey: .workspaceDisplayName)
    }
}

public enum HUDDisplayField: String, CaseIterable, Codable, Sendable, Hashable, Identifiable {
    case provider
    case event
    case task
    case project
    case elapsed

    public var id: Self { self }
}

public struct HUDSettings: Codable, Equatable, Sendable {
    public var isEnabled: Bool
    public var displaySeconds: Int
    public var fields: [HUDDisplayField]

    public init(
        isEnabled: Bool = true,
        displaySeconds: Int = 8,
        fields: [HUDDisplayField] = [.provider, .event, .task, .elapsed]
    ) {
        self.isEnabled = isEnabled
        self.displaySeconds = displaySeconds
        self.fields = fields
    }

    public static let defaults = HUDSettings()
}

/// macOS-only visual preference. Selected media is copied into ActRealm's
/// Application Support directory so the background remains available after
/// the original file is moved or temporary picker access ends.
public enum ThemeBackgroundKind: String, Codable, Equatable, Sendable {
    case image
    case animatedImage
    case video
}

public struct AppThemeSettings: Codable, Equatable, Sendable {
    public var customBackgroundPath: String?
    public var backgroundKind: ThemeBackgroundKind
    /// 0 is completely transparent; 1 is completely opaque.
    public var laneOpacity: Double

    public init(
        customBackgroundPath: String? = nil,
        backgroundKind: ThemeBackgroundKind = .image,
        laneOpacity: Double = 0.55
    ) {
        self.customBackgroundPath = customBackgroundPath
        self.backgroundKind = backgroundKind
        self.laneOpacity = laneOpacity
    }

    public static let defaults = AppThemeSettings()

    private enum CodingKeys: String, CodingKey {
        case customBackgroundPath, backgroundKind, laneOpacity, laneTransparency
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        customBackgroundPath = try values.decodeIfPresent(
            String.self,
            forKey: .customBackgroundPath
        )
        backgroundKind = try values.decodeIfPresent(
            ThemeBackgroundKind.self,
            forKey: .backgroundKind
        ) ?? .image
        if let opacity = try values.decodeIfPresent(Double.self, forKey: .laneOpacity) {
            laneOpacity = opacity
        } else if let legacyTransparency = try values.decodeIfPresent(
            Double.self,
            forKey: .laneTransparency
        ) {
            laneOpacity = 1 - legacyTransparency
        } else {
            laneOpacity = 0.55
        }
    }

    public func encode(to encoder: Encoder) throws {
        var values = encoder.container(keyedBy: CodingKeys.self)
        try values.encodeIfPresent(customBackgroundPath, forKey: .customBackgroundPath)
        try values.encode(backgroundKind, forKey: .backgroundKind)
        try values.encode(laneOpacity, forKey: .laneOpacity)
    }
}

public enum ForegroundDispatchPhase: Equatable, Sendable {
    case reminding
    case opening
    case awaitingWorkspace
    case returnedToActRealmWorkspace
}

/// One live arrival moving through the policy selected on the 台前调度 page.
/// The UI renders this as the reminder/opening/acceptance HUD while AppKit owns
/// the actual foreground application and Space observations.
public struct ForegroundDispatchState: Identifiable, Equatable, Sendable {
    public let id: String
    public let provider: ProviderKind?
    public let title: String
    public let taskTitle: String?
    public let phase: ForegroundDispatchPhase
    public let startedAt: Date
    public let deadline: Date

    public init(
        id: String,
        provider: ProviderKind?,
        title: String,
        taskTitle: String?,
        phase: ForegroundDispatchPhase,
        startedAt: Date,
        deadline: Date
    ) {
        self.id = id
        self.provider = provider
        self.title = title
        self.taskTitle = taskTitle
        self.phase = phase
        self.startedAt = startedAt
        self.deadline = deadline
    }
}

public struct ForegroundWorkspaceStatus: Equatable, Sendable {
    public var isActRealmWorkspaceReady: Bool
    public var isAgentAvailable: Bool
    public var isSchedulingWorkspaceActive: Bool

    public init(
        isActRealmWorkspaceReady: Bool = true,
        isAgentAvailable: Bool = true,
        isSchedulingWorkspaceActive: Bool = false
    ) {
        self.isActRealmWorkspaceReady = isActRealmWorkspaceReady
        self.isAgentAvailable = isAgentAvailable
        self.isSchedulingWorkspaceActive = isSchedulingWorkspaceActive
    }
}

/// Top-level observable state for every scene (main window, HUD, menu bar,
/// settings). Owns the runtime supervisor + client and projects snapshots
/// into `DerivedState`.
@MainActor
public final class AppModel: ObservableObject {
    @Published public private(set) var derived: DerivedState = .empty
    @Published public private(set) var bridgeStatus: BridgeStatus = .starting
    @Published public private(set) var lastSyncAt: Date?
    @Published public private(set) var runtimeDiagnostics: RuntimeSupervisor.Diagnostics = .empty
    @Published public private(set) var isRestartingRuntime = false
    @Published public private(set) var runtimeActionMessage: String?
    @Published public private(set) var toastMessage: String?
    @Published public private(set) var isHUDPreviewActive = false
    /// Ticks once per second so waiting timers and countdown rings advance.
    @Published public private(set) var now: Date = Date()
    @Published public private(set) var setupInfo: SetupInfo?
    @Published public private(set) var uiSettings: UISettings = .defaults
    @Published public private(set) var displayCatalog: [DisplayField] = []
    @Published public private(set) var claudeQuotaBridge: ClaudeQuotaBridge?
    @Published public private(set) var isSetupBusy = false
    @Published public private(set) var isSettingsBusy = false
    @Published public private(set) var eventUIP95Ms: UInt64?
    /// Monotonic event observed by the AppKit shell to play the optional local
    /// arrival sound without moving platform UI code into ActRealmKit.
    @Published public private(set) var notificationPulse: UInt64 = 0
    @Published public private(set) var hudSettings: HUDSettings
    @Published public private(set) var hudArrivalID: String?
    @Published public private(set) var hudArrivalDeadline: Date?
    @Published public private(set) var themeSettings: AppThemeSettings
    /// Live main-window aspect ratio used by Settings to preview the same
    /// scaled-to-fill crop the user will see in the workspace.
    @Published public private(set) var mainWindowAspectRatio = 1440.0 / 820.0

    /// Index of the approval card currently shown in the OUTBOX stack.
    @Published public var outboxPageIndex: Int = 0
    /// Session pinned by tapping an OUTBOX card (spec §3.7 cross-column link).
    @Published public var pinnedSessionId: String?
    /// Expanded row in the interaction model's AGENT TASKS column.
    @Published public var expandedTaskId: String?
    /// Brief cross-column emphasis used by the prototype when a task links
    /// back to its pending OUTBOX item.
    @Published public private(set) var outboxHighlighted = false
    @Published public private(set) var foregroundScheduling: ForegroundSchedulingSettings
    @Published public private(set) var foregroundDispatch: ForegroundDispatchState?
    @Published public private(set) var foregroundReturnNotes: [String: String] = [:]
    @Published public private(set) var foregroundSuppressedAttentionIDs: Set<String> = []
    @Published public private(set) var foregroundWorkspaceStatus = ForegroundWorkspaceStatus()
    @Published public private(set) var isSelectingForegroundWorkspace = false

    public let isDemo: Bool
    public let supervisor: RuntimeSupervisor
    public let client: RuntimeClient

    public var isFirstRun: Bool { setupInfo?.firstRun == true }
    public var connectedAgentCount: Int {
        setupInfo?.providers.filter { $0.status == "connected" }.count ?? 0
    }
    public var pendingAgentSetupCount: Int {
        setupInfo?.providers.filter {
            !["connected", "not_installed", "provider_missing", "cli_missing"].contains($0.status)
        }.count ?? 0
    }

    private static let legacyApprovalPolicyKey = "actrealm.approvalPolicy"
    private static let dismissedTasksKey = "actrealm.dismissedTaskVersions"
    private static let foregroundTestDispatchID = "foreground-scheduling-test"
    private static let foregroundSchedulingKey = "actrealm.foregroundScheduling"
    private static let hudSettingsKey = "actrealm.hudSettings"
    private static let themeSettingsKey = "actrealm.themeSettings"
    private let defaults: UserDefaults
    private let themeDirectory: URL
    private var cancellables: Set<AnyCancellable> = []
    private var ticker: Task<Void, Never>?
    private var dismissedTaskVersions: [String: UInt64]
    private var toastTask: Task<Void, Never>?
    private var outboxHighlightTask: Task<Void, Never>?
    private var hudPreviewTask: Task<Void, Never>?
    private var settingsSaveTask: Task<Void, Never>?
    private var setupRefreshTask: Task<Void, Never>?
    private var seededForegroundArrivals = false
    private var seededNotifications = false
    private var lastSetupRefreshEventCount: UInt64 = 0
    private var latestSnapshot: Snapshot = .empty
    private var persistedUISettings: UISettings = .defaults
    private var renderedEventCount: UInt64 = 0
    private var eventUILatenciesMs: [UInt64] = []
    private var started = false

    public init(
        repoPath: URL? = nil,
        defaults: UserDefaults = .standard,
        themeDirectory: URL? = nil,
        demo: Bool = ProcessInfo.processInfo.environment["ACTREALM_DEMO"] == "1"
    ) {
        self.defaults = defaults
        self.themeDirectory = themeDirectory
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("ActRealm/Theme", isDirectory: true)
        self.isDemo = demo
        if !demo,
           let data = defaults.data(forKey: Self.foregroundSchedulingKey),
           let settings = try? JSONDecoder().decode(ForegroundSchedulingSettings.self, from: data)
        {
            self.foregroundScheduling = settings
        } else {
            self.foregroundScheduling = .defaults
        }
        if !demo,
           let data = defaults.data(forKey: Self.hudSettingsKey),
           let settings = try? JSONDecoder().decode(HUDSettings.self, from: data)
        {
            self.hudSettings = settings
        } else {
            self.hudSettings = .defaults
        }
        if !demo,
           let data = defaults.data(forKey: Self.themeSettingsKey),
           var settings = try? JSONDecoder().decode(AppThemeSettings.self, from: data)
        {
            if let path = settings.customBackgroundPath,
               !FileManager.default.fileExists(atPath: path)
            {
                settings.customBackgroundPath = nil
                settings.backgroundKind = .image
            }
            settings.laneOpacity = Self.clampedLaneOpacity(settings.laneOpacity)
            self.themeSettings = settings
        } else {
            self.themeSettings = .defaults
        }
        self.hudArrivalID = nil
        self.hudArrivalDeadline = nil
        self.foregroundDispatch = nil
        let supervisor = RuntimeSupervisor(repoPath: repoPath)
        self.supervisor = supervisor
        self.client = RuntimeClient()
        // The Web/native shared interaction model has no global auto-approval
        // mode. Remove the legacy native-only preference so it can never make
        // an invisible decision after the old settings control disappeared.
        defaults.removeObject(forKey: Self.legacyApprovalPolicyKey)
        self.dismissedTaskVersions = (defaults.dictionary(forKey: Self.dismissedTasksKey) ?? [:])
            .reduce(into: [:]) { result, pair in
                if let value = pair.value as? NSNumber {
                    result[pair.key] = value.uint64Value
                }
            }

        client.$snapshot
            .receive(on: RunLoop.main)
            .sink { [weak self] snapshot in
                self?.apply(snapshot: snapshot)
            }
            .store(in: &cancellables)
        client.$connectionState
            .combineLatest(supervisor.$state)
            .receive(on: RunLoop.main)
            .sink { [weak self] connection, backend in
                self?.updateBridgeStatus(connection: connection, backend: backend)
            }
            .store(in: &cancellables)
        supervisor.$diagnostics
            .receive(on: RunLoop.main)
            .sink { [weak self] diagnostics in
                self?.runtimeDiagnostics = diagnostics
            }
            .store(in: &cancellables)
    }

    deinit {
        ticker?.cancel()
        toastTask?.cancel()
        outboxHighlightTask?.cancel()
        hudPreviewTask?.cancel()
        settingsSaveTask?.cancel()
        setupRefreshTask?.cancel()
    }

    // MARK: - Lifecycle

    public func start() {
        guard !started else { return }
        started = true
        startTicker()
        if isDemo {
            derived = DemoData.derivedState(now: now)
            setupInfo = DemoData.setup
            uiSettings = DemoData.settings.settings
            persistedUISettings = DemoData.settings.settings
            displayCatalog = DemoData.settings.displayCatalog
            claudeQuotaBridge = DemoData.settings.claudeQuotaBridge
            bridgeStatus = .listening
            lastSyncAt = now
            return
        }
        let supervisor = self.supervisor
        let client = self.client
        Task {
            supervisor.refreshDiagnostics()
            await supervisor.start { baseURL, token in
                Task {
                    await client.connect(baseURL: baseURL, token: token)
                    await self.refreshSetup()
                    await self.refreshSettings()
                    await client.recordMetric("app_opened")
                }
            }
        }
    }

    public func shutdown() {
        client.disconnect()
        supervisor.stop()
        ticker?.cancel()
    }

    public func refreshRuntimeDiagnostics() {
        supervisor.refreshDiagnostics()
    }

    public func restartRuntime() {
        guard !isDemo, !isRestartingRuntime else { return }
        isRestartingRuntime = true
        runtimeActionMessage = nil
        let supervisor = self.supervisor
        let client = self.client
        Task {
            client.disconnect()
            let error = await supervisor.restart { baseURL, token in
                Task {
                    await client.connect(baseURL: baseURL, token: token)
                    await self.refreshSetup()
                    await self.refreshSettings()
                }
            }
            if let error {
                runtimeActionMessage = error
                isRestartingRuntime = false
                return
            }

            for _ in 0..<60 {
                if bridgeStatus.isListening { break }
                if case .failed(let message) = supervisor.state {
                    runtimeActionMessage = message
                    break
                }
                try? await Task.sleep(for: .milliseconds(200))
            }
            supervisor.refreshDiagnostics()
            if bridgeStatus.isListening {
                runtimeActionMessage = "Runtime 已重新启动并恢复连接"
            } else if runtimeActionMessage == nil {
                runtimeActionMessage = "Runtime 已启动，但控制连接尚未恢复"
            }
            isRestartingRuntime = false
        }
    }

    // MARK: - Shared Web/native control-surface state

    public func refreshSetup() async {
        guard !isDemo else { return }
        if let setup = await client.fetchSetup() {
            setupInfo = setup
            apply(snapshot: latestSnapshot, emitArrivals: false)
        }
    }

    @discardableResult
    public func changeSetup(provider: String, action: String) async -> Bool {
        guard !isDemo, !isSetupBusy else { return false }
        isSetupBusy = true
        defer { isSetupBusy = false }
        let (updated, error) = await client.changeSetup(
            provider: provider,
            action: action,
            enhancedCodexActivity: uiSettings.codexEnhancedActivity
        )
        if let updated {
            setupInfo = updated
            apply(snapshot: latestSnapshot, emitArrivals: false)
            showToast(action == "uninstall"
                ? "\(providerDisplayName(provider)) 接入已移除"
                : "\(providerDisplayName(provider)) 配置已安全写入")
            return true
        }
        showToast("接入操作失败：\(error ?? "未知错误")")
        return false
    }

    public func refreshSettings() async {
        guard !isDemo else { return }
        guard let response = await client.fetchSettings() else { return }
        acceptSettings(response)
    }

    /// Updates the local view immediately, then persists the complete Runtime
    /// settings object. A short debounce keeps segmented controls responsive
    /// while preserving the server's all-fields validation contract.
    public func updateUISettings(_ update: (inout UISettings) -> Void) {
        var next = uiSettings
        update(&next)
        guard next != uiSettings else { return }
        uiSettings = next
        apply(snapshot: latestSnapshot, emitArrivals: false)
        settingsSaveTask?.cancel()
        settingsSaveTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(180))
            guard let self, !Task.isCancelled else { return }
            await self.persistSettings(next)
        }
    }

    private func persistSettings(_ settings: UISettings) async {
        guard !isDemo else { return }
        let previousCodexMode = persistedUISettings.codexEnhancedActivity
        isSettingsBusy = true
        defer { isSettingsBusy = false }
        let (response, error) = await client.updateSettings(settings)
        if let response {
            acceptSettings(response)
            if previousCodexMode != response.settings.codexEnhancedActivity {
                showToast("Codex Hook 已更新，请在 Codex 中运行 /hooks 重新检查信任")
                await refreshSetup()
            } else {
                showToast("设置已保存到本机")
            }
        } else {
            showToast("设置保存失败：\(error ?? "未知错误")")
            await refreshSettings()
        }
    }

    private func acceptSettings(_ response: SettingsResponse) {
        uiSettings = response.settings
        persistedUISettings = response.settings
        displayCatalog = response.displayCatalog
        claudeQuotaBridge = response.claudeQuotaBridge
        apply(snapshot: latestSnapshot, emitArrivals: false)
    }

    public func changeClaudeQuotaBridge(action: String) async {
        guard !isDemo, !isSettingsBusy else { return }
        isSettingsBusy = true
        defer { isSettingsBusy = false }
        let (response, error) = await client.changeClaudeQuotaBridge(action: action)
        if let response {
            acceptSettings(response)
            await client.refreshSnapshot()
            showToast(action == "uninstall"
                ? "Claude 额度桥已关闭，原状态栏已恢复"
                : "Claude 额度桥已开启，完成一次对话后会显示额度")
        } else {
            showToast("额度桥操作失败：\(error ?? "未知错误")")
        }
    }

    public func exportLocalData(metricsOnly: Bool) async -> Data? {
        let (data, error) = await client.exportData(metricsOnly: metricsOnly)
        if data == nil { showToast("导出失败：\(error ?? "未知错误")") }
        return data
    }

    @discardableResult
    public func clearLocalData(confirmation: String) async -> Bool {
        guard confirmation == "DELETE" else {
            showToast("请输入 DELETE；没有删除任何数据")
            return false
        }
        if let error = await client.clearData(confirmation: confirmation) {
            showToast("清除失败：\(error)")
            return false
        }
        await refreshSettings()
        showToast("本地运行数据已彻底清除，Hook 接入保持不变")
        return true
    }

    public func submitQuestion(
        _ entry: OutboxEntry,
        action: String,
        answers: [String: JSONValue]? = nil
    ) async -> Bool {
        guard let requestId = entry.attention.requestId else {
            showToast("这个问题没有可用的回复通道")
            return false
        }
        if let error = await client.answerQuestion(
            requestId: requestId,
            action: action,
            answers: answers
        ) {
            showToast("回答失败：\(error)")
            return false
        }
        showToast(action == "native" ? "已交回 Agent 原界面回答" : "回答已安全发送给 Agent")
        return true
    }

    public func jump(to task: LaneTask) async {
        guard task.session.jumpCapability != "unsupported" else {
            showToast("当前环境不支持跳转；ActRealm 不会假装已定位到原对话")
            return
        }
        let (response, error) = await client.jumpSession(task.id)
        if let response, response.success {
            showToast(response.label)
        } else {
            showToast("跳转失败：\(error ?? "没有找到原窗口")")
        }
    }

    public func jump(to entry: OutboxEntry) async {
        guard let task = derived.agentTasks.first(where: { $0.id == entry.attention.sessionId }) else {
            revealSession(for: entry)
            showToast("已定位对应任务；当前没有可验证的原窗口跳转信息")
            return
        }
        await jump(to: task)
    }

    public func manage(_ task: LaneTask) async {
        guard task.session.canManage == true else { return }
        if let error = await client.manageSession(task.id) {
            showToast("托管连接失败：\(error)")
        } else {
            showToast("Codex 对话已由 ActRealm app-server Connector 接管")
        }
    }

    private func providerDisplayName(_ provider: String) -> String {
        switch provider {
        case "claude": "Claude"
        case "codex": "Codex"
        default: provider
        }
    }

    private func startTicker() {
        ticker?.cancel()
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                self.now = Date()
                self.advanceForegroundDispatch(at: self.now)
                if let deadline = self.hudArrivalDeadline, self.now >= deadline {
                    self.hudArrivalID = nil
                    self.hudArrivalDeadline = nil
                }
                if self.isDemo {
                    self.derived = DemoData.derivedState(now: self.now)
                }
            }
        }
    }

    // MARK: - Snapshot handling

    private func apply(snapshot: Snapshot, emitArrivals: Bool = true) {
        latestSnapshot = snapshot
        let allowedAttentionIDs = Set(snapshot.attention.filter {
            uiSettings.notificationRules.mode(for: $0.kind) != .ignore
        }.map(\.id))
        // Until setup is resolved, and whenever both supported Providers are
        // genuinely unconfigured, do not flash restored/history rows as a
        // current workspace. This mirrors Web's first-run render contract.
        let complete = setupInfo == nil || setupInfo?.firstRun == true
            ? DerivedState.empty
            : DerivedState.derive(from: snapshot)
        // Reminder rules control presentation, not Runtime truth. An ignored
        // approval still keeps its task in the waiting state, matching Web.
        let next = DerivedState(
            outbox: complete.outbox.filter { allowedAttentionIDs.contains($0.id) },
            lanes: complete.lanes,
            quotaSlots: complete.quotaSlots,
            pendingDecision: complete.pendingDecision
        )
        let nextOpenIDs = Set(next.openOutbox.map(\.id))
        foregroundReturnNotes = foregroundReturnNotes.filter { nextOpenIDs.contains($0.key) }
        foregroundSuppressedAttentionIDs.formIntersection(nextOpenIDs)
        if let dispatch = foregroundDispatch,
           dispatch.id != Self.foregroundTestDispatchID,
           !nextOpenIDs.contains(dispatch.id)
        {
            foregroundDispatch = nil
        }

        if emitArrivals, seededForegroundArrivals {
            let previousIDs = Set(derived.openOutbox.map(\.id))
            let arrivals = next.openOutbox
                .filter { $0.state == .open && !previousIDs.contains($0.id) }
                .sorted { $0.createdAt < $1.createdAt }
            for arrival in arrivals {
                receiveForegroundArrival(
                    id: arrival.id,
                    provider: arrival.provider,
                    title: arrival.actionTitle,
                    taskTitle: arrival.taskTitle
                )
            }
            if hudSettings.isEnabled, let arrival = arrivals.last {
                hudArrivalID = arrival.id
                hudArrivalDeadline = Date().addingTimeInterval(TimeInterval(hudSettings.displaySeconds))
            }
            if seededNotifications, uiSettings.soundEnabled, !arrivals.isEmpty {
                notificationPulse &+= 1
            }
        } else if emitArrivals {
            seededForegroundArrivals = true
        }
        if emitArrivals { seededNotifications = true }

        derived = next
        if emitArrivals { lastSyncAt = Date() }
        if emitArrivals, snapshot.stats.eventCount > renderedEventCount {
            let latestEventAt = snapshot.sessions.map(\.lastEventAt).max() ?? 0
            let currentMillis = UInt64(max(0, Date().timeIntervalSince1970 * 1000))
            if latestEventAt > 0, currentMillis >= latestEventAt {
                let latency = currentMillis - latestEventAt
                if latency <= 10_000 {
                    eventUILatenciesMs.append(latency)
                    if eventUILatenciesMs.count > 100 {
                        eventUILatenciesMs.removeFirst(eventUILatenciesMs.count - 100)
                    }
                    let sorted = eventUILatenciesMs.sorted()
                    let index = max(0, Int(ceil(Double(sorted.count) * 0.95)) - 1)
                    eventUIP95Ms = sorted[index]
                }
            }
            renderedEventCount = snapshot.stats.eventCount
        }
        clampOutboxPage()
        if snapshot.stats.eventCount != lastSetupRefreshEventCount {
            lastSetupRefreshEventCount = snapshot.stats.eventCount
            scheduleSetupRefresh()
        }
    }

    private func scheduleSetupRefresh() {
        setupRefreshTask?.cancel()
        setupRefreshTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(500))
            guard let self, !Task.isCancelled else { return }
            await self.refreshSetup()
        }
    }

    private func updateBridgeStatus(
        connection: RuntimeClient.ConnectionState,
        backend: RuntimeSupervisor.State
    ) {
        if isDemo { return }
        switch (backend, connection) {
        case (_, .live):
            bridgeStatus = .listening
        case (.failed(let message), _):
            bridgeStatus = .absent(message)
        case (.stopped, _):
            bridgeStatus = .absent("Runtime 已退出")
        case (.buildingBackend, _), (.launching, _), (.restarting, _), (.idle, _), (_, .connecting), (_, .idle):
            bridgeStatus = .starting
        case (_, .error(let message)):
            bridgeStatus = .absent(message)
        }
    }

    private func clampOutboxPage() {
        let count = derived.openOutbox.count
        if outboxPageIndex >= count {
            outboxPageIndex = max(0, count - 1)
        }
    }

    // MARK: - User actions

    public func approve(_ entry: OutboxEntry) {
        send(.approve, entry: entry)
    }

    public func deny(_ entry: OutboxEntry) {
        send(.deny, entry: entry)
    }

    /// "去原窗口核对 / 处理" — hands the decision back to the CLI window.
    public func passThrough(_ entry: OutboxEntry) {
        send(.passThrough, entry: entry)
    }

    /// "标记已处理 / 确认完成" for question, error, completion items.
    public func acknowledge(_ entry: OutboxEntry) {
        send(.ack, entry: entry)
    }

    public func snooze(_ entry: OutboxEntry) {
        send(.snooze, entry: entry)
    }

    public func undoPendingDecision() {
        guard let pending = derived.pendingDecision, case .undoable = pending.phase else { return }
        guard !isDemo else { return }
        Task { await client.undo(commandId: pending.commandId) }
    }

    /// Cross-column link: pin the session belonging to an OUTBOX entry.
    public func revealSession(for entry: OutboxEntry) {
        pinnedSessionId = entry.attention.sessionId
        expandedTaskId = entry.attention.sessionId
    }

    /// Cross-column link: jump the OUTBOX pager to a task's first open item.
    public func revealOutbox(for task: LaneTask) {
        guard let target = task.firstOpenOutboxId,
              let index = derived.openOutbox.firstIndex(where: { $0.id == target })
        else { return }
        outboxPageIndex = index
        outboxHighlighted = true
        showToast("已定位到 OUTBOX 待处理事项")
        outboxHighlightTask?.cancel()
        outboxHighlightTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(1_600))
            guard !Task.isCancelled else { return }
            self?.outboxHighlighted = false
        }
        pinnedSessionId = task.session.id
        expandedTaskId = task.session.id
    }

    /// Matches the Web task-clear behavior: hand questions back to the Agent,
    /// dismiss other related presentation items, then hide this session
    /// version. Any later Runtime event makes it visible again.
    public func dismissTask(_ task: LaneTask) {
        Task { await clearTask(task) }
    }

    private func clearTask(_ task: LaneTask) async {
        let related = derived.openOutbox.filter { $0.attention.sessionId == task.id }
        var failed = 0
        if !isDemo {
            for entry in related {
                if await client.dismissAttention(entry.attention) != nil { failed += 1 }
            }
        }
        dismissedTaskVersions[task.id] = task.session.lastEventAt
        defaults.set(
            dismissedTaskVersions.mapValues { NSNumber(value: $0) },
            forKey: Self.dismissedTasksKey
        )
        if expandedTaskId == task.id { expandedTaskId = nil }
        if pinnedSessionId == task.id { pinnedSessionId = nil }
        if failed > 0 {
            showToast("任务已清除；\(failed) 项仍需在 OUTBOX 或 Agent 原界面处理")
        } else if !related.isEmpty {
            showToast("任务已清除，并交还 \(related.count) 项待处理事项")
        } else {
            showToast("已从列表移除；有新活动时会自动恢复")
        }
    }

    public func isTaskDismissed(_ task: LaneTask) -> Bool {
        guard let dismissedAt = dismissedTaskVersions[task.id] else { return false }
        return dismissedAt >= task.session.lastEventAt
    }

    public func showToast(_ message: String) {
        toastTask?.cancel()
        toastMessage = message
        toastTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(2.4))
            guard !Task.isCancelled else { return }
            self?.toastMessage = nil
        }
    }

    /// Applies one atomic edit and persists it immediately. The page calls this
    /// for every card, duration, switch, and provider-rule interaction so the
    /// preview and the policy consumed by future task arrivals never diverge.
    public func updateForegroundScheduling(
        _ update: (inout ForegroundSchedulingSettings) -> Void
    ) {
        var settings = foregroundScheduling
        update(&settings)
        settings.reminderSeconds = Self.supportedSeconds(settings.reminderSeconds)
        settings.acceptanceSeconds = Self.supportedSeconds(settings.acceptanceSeconds)
        guard settings != foregroundScheduling else { return }
        foregroundScheduling = settings
        guard !isDemo, let data = try? JSONEncoder().encode(settings) else { return }
        defaults.set(data, forKey: Self.foregroundSchedulingKey)
    }

    public func beginForegroundWorkspaceSelection() {
        isSelectingForegroundWorkspace = true
    }

    public func cancelForegroundWorkspaceSelection() {
        isSelectingForegroundWorkspace = false
    }

    public func bindForegroundWorkspace(apps: [ForegroundWorkspaceApp]) {
        bindForegroundWorkspace(apps: apps, displayID: nil, displayName: nil)
    }

    public func bindForegroundWorkspace(
        apps: [ForegroundWorkspaceApp],
        displayID: UInt32?,
        displayName: String?
    ) {
        guard !apps.isEmpty else {
            showToast("当前桌面没有可绑定的协作应用")
            return
        }
        updateForegroundScheduling {
            $0.workspaceApps = apps
            $0.workspaceBoundAt = Date()
            $0.workspaceDisplayID = displayID
            $0.workspaceDisplayName = displayName
        }
        isSelectingForegroundWorkspace = false
        showToast("协作桌面已绑定")
    }

    public func clearForegroundWorkspaceBinding() {
        updateForegroundScheduling {
            $0.workspaceApps = []
            $0.workspaceBoundAt = nil
            $0.workspaceDisplayID = nil
            $0.workspaceDisplayName = nil
        }
        showToast("协作桌面绑定已清除")
    }

    public var themeBackgroundURL: URL? {
        guard let path = themeSettings.customBackgroundPath,
              FileManager.default.fileExists(atPath: path)
        else { return nil }
        return URL(fileURLWithPath: path)
    }

    public func importThemeBackground(
        from sourceURL: URL,
        kind: ThemeBackgroundKind
    ) throws {
        let hasScopedAccess = sourceURL.startAccessingSecurityScopedResource()
        defer {
            if hasScopedAccess { sourceURL.stopAccessingSecurityScopedResource() }
        }

        let fileManager = FileManager.default
        try fileManager.createDirectory(
            at: themeDirectory,
            withIntermediateDirectories: true
        )
        let sourceExtension = sourceURL.pathExtension.lowercased()
        let safeExtension = sourceExtension.isEmpty ? "image" : sourceExtension
        let destination = themeDirectory.appendingPathComponent(
            "background-\(UUID().uuidString).\(safeExtension)"
        )
        try fileManager.copyItem(at: sourceURL, to: destination)

        let previousURL = themeBackgroundURL
        themeSettings.customBackgroundPath = destination.path
        themeSettings.backgroundKind = kind
        persistThemeSettings()
        if let previousURL,
           previousURL.standardizedFileURL.deletingLastPathComponent()
            == themeDirectory.standardizedFileURL
        {
            try? fileManager.removeItem(at: previousURL)
        }
    }

    public func resetThemeBackground() {
        if let currentURL = themeBackgroundURL,
           currentURL.standardizedFileURL.deletingLastPathComponent()
            == themeDirectory.standardizedFileURL
        {
            try? FileManager.default.removeItem(at: currentURL)
        }
        themeSettings.customBackgroundPath = nil
        themeSettings.backgroundKind = .image
        persistThemeSettings()
    }

    public func updateThemeSettings(_ update: (inout AppThemeSettings) -> Void) {
        var settings = themeSettings
        update(&settings)
        settings.laneOpacity = Self.clampedLaneOpacity(settings.laneOpacity)
        guard settings != themeSettings else { return }
        themeSettings = settings
        persistThemeSettings()
    }

    public func updateMainWindowSize(_ size: CGSize) {
        guard size.width > 0, size.height > 0 else { return }
        let ratio = Double(size.width / size.height)
        guard abs(ratio - mainWindowAspectRatio) > 0.001 else { return }
        mainWindowAspectRatio = ratio
    }

    private static func clampedLaneOpacity(_ value: Double) -> Double {
        min(1, max(0, value))
    }

    private func persistThemeSettings() {
        guard !isDemo, let data = try? JSONEncoder().encode(themeSettings) else { return }
        defaults.set(data, forKey: Self.themeSettingsKey)
    }

    public func updateHUDSettings(_ update: (inout HUDSettings) -> Void) {
        var settings = hudSettings
        update(&settings)
        settings.displaySeconds = Self.supportedHUDSeconds(settings.displaySeconds)
        settings.fields = HUDDisplayField.allCases.filter { settings.fields.contains($0) }
        guard settings != hudSettings else { return }
        hudSettings = settings
        if !settings.isEnabled {
            hudArrivalID = nil
            hudArrivalDeadline = nil
        }
        guard !isDemo, let data = try? JSONEncoder().encode(settings) else { return }
        defaults.set(data, forKey: Self.hudSettingsKey)
    }

    private static func supportedHUDSeconds(_ seconds: Int) -> Int {
        [5, 8, 12, 20].min(by: { abs($0 - seconds) < abs($1 - seconds) }) ?? 8
    }

    private static func supportedSeconds(_ seconds: Int) -> Int {
        [5, 10, 30].min(by: { abs($0 - seconds) < abs($1 - seconds) }) ?? 10
    }

    public func effectiveForegroundStrategy(for provider: ProviderKind?) -> ForegroundArrivalStrategy {
        guard foregroundScheduling.isEnabled else { return .actRealmWorkspace }
        let rule: ForegroundAgentRule
        switch provider {
        case .codex: rule = foregroundScheduling.codexRule
        case .claude: rule = foregroundScheduling.claudeRule
        case .gemini, .custom, nil: rule = .defaultRule
        }
        switch rule {
        case .defaultRule: return foregroundScheduling.strategy
        case .immediate: return .immediate
        case .remind: return .remind
        case .actRealmWorkspace: return .actRealmWorkspace
        }
    }

    public func openForegroundAgentNow() {
        guard let dispatch = foregroundDispatch else { return }
        beginOpening(dispatch, at: Date())
    }

    public func keepForegroundTaskInActRealmWorkspace() {
        guard foregroundDispatch != nil else { return }
        foregroundDispatch = nil
        showToast("任务已留在 ActRealm 工作区的待处理列表")
    }

    /// Deterministic visual preview used by SnapshotTool. It never creates a
    /// Runtime attention item or sends a provider command.
    public func previewForegroundScheduling() {
        guard isDemo else { return }
        testForegroundScheduling()
    }

    /// Exercises the complete AppKit scheduling path without creating a
    /// Runtime attention item or sending a provider command.
    public func testForegroundScheduling() {
        foregroundDispatch = nil
        receiveForegroundArrival(
            id: Self.foregroundTestDispatchID,
            provider: .claude,
            title: "台前调度测试",
            taskTitle: "验证协作桌面切换与接收",
            at: Date()
        )
    }

    public func acceptForegroundWorkspace() {
        guard let dispatch = foregroundDispatch,
              dispatch.phase == .awaitingWorkspace
        else { return }
        foregroundDispatch = nil
        if foregroundScheduling.closesAfterAcceptance {
            updateForegroundScheduling { $0.isEnabled = false }
            showToast("已接收 · 台前调度已自动关闭")
        } else {
            showToast("已接收 · 返回倒计时已停止")
        }
    }

    public func isForegroundHUDSuppressed(for attentionID: String) -> Bool {
        foregroundSuppressedAttentionIDs.contains(attentionID)
    }

    public func updateForegroundWorkspaceStatus(_ status: ForegroundWorkspaceStatus) {
        guard status != foregroundWorkspaceStatus else { return }
        foregroundWorkspaceStatus = status
    }

    /// Internal entry point shared by live snapshot arrivals and focused tests.
    func receiveForegroundArrival(
        id: String,
        provider: ProviderKind?,
        title: String,
        taskTitle: String?,
        at date: Date = Date()
    ) {
        guard foregroundDispatch == nil else { return }

        switch effectiveForegroundStrategy(for: provider) {
        case .actRealmWorkspace:
            showToast("新任务已进入 ActRealm 工作区的待处理列表")
        case .immediate:
            guard !foregroundScheduling.workspaceApps.isEmpty else {
                showToast("新任务已进入待处理列表；绑定协作桌面后可自动打开")
                return
            }
            foregroundSuppressedAttentionIDs.insert(id)
            foregroundDispatch = ForegroundDispatchState(
                id: id,
                provider: provider,
                title: title,
                taskTitle: taskTitle,
                phase: .opening,
                startedAt: date,
                deadline: date.addingTimeInterval(1.2)
            )
        case .remind:
            guard !foregroundScheduling.workspaceApps.isEmpty else {
                showToast("新任务已进入待处理列表；绑定协作桌面后可自动打开")
                return
            }
            foregroundSuppressedAttentionIDs.insert(id)
            foregroundDispatch = ForegroundDispatchState(
                id: id,
                provider: provider,
                title: title,
                taskTitle: taskTitle,
                phase: .reminding,
                startedAt: date,
                deadline: date.addingTimeInterval(TimeInterval(foregroundScheduling.reminderSeconds))
            )
        }
    }

    func advanceForegroundDispatch(at date: Date) {
        guard let dispatch = foregroundDispatch, date >= dispatch.deadline else { return }
        switch dispatch.phase {
        case .reminding:
            beginOpening(dispatch, at: date)
        case .opening:
            guard foregroundScheduling.returnsToActRealmWorkspace else {
                foregroundDispatch = nil
                return
            }
            foregroundDispatch = ForegroundDispatchState(
                id: dispatch.id,
                provider: dispatch.provider,
                title: dispatch.title,
                taskTitle: dispatch.taskTitle,
                phase: .awaitingWorkspace,
                startedAt: date,
                deadline: date.addingTimeInterval(TimeInterval(foregroundScheduling.acceptanceSeconds))
            )
        case .awaitingWorkspace:
            let note = "未检测到进入调度工作桌面，已返回"
            foregroundReturnNotes[dispatch.id] = note
            foregroundDispatch = ForegroundDispatchState(
                id: dispatch.id,
                provider: dispatch.provider,
                title: dispatch.title,
                taskTitle: dispatch.taskTitle,
                phase: .returnedToActRealmWorkspace,
                startedAt: date,
                deadline: date.addingTimeInterval(1.4)
            )
            showToast(note)
        case .returnedToActRealmWorkspace:
            foregroundDispatch = nil
        }
    }

    private func beginOpening(_ dispatch: ForegroundDispatchState, at date: Date) {
        foregroundDispatch = ForegroundDispatchState(
            id: dispatch.id,
            provider: dispatch.provider,
            title: dispatch.title,
            taskTitle: dispatch.taskTitle,
            phase: .opening,
            startedAt: date,
            deadline: date.addingTimeInterval(1.2)
        )
    }

    /// Visual-only HUD preview used by Runtime Monitor. It never creates an
    /// attention item and cannot send an approval command.
    public func previewHUD() {
        isHUDPreviewActive = true
        hudPreviewTask?.cancel()
        let seconds = hudSettings.displaySeconds
        hudPreviewTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(seconds))
            guard !Task.isCancelled else { return }
            self?.isHUDPreviewActive = false
        }
    }

    private func send(_ action: AttentionAction, entry: OutboxEntry) {
        guard !isDemo else { return }
        let requestId = entry.attention.requestId
        Task { await client.send(action: action, attentionId: entry.id, requestId: requestId) }
    }
}
