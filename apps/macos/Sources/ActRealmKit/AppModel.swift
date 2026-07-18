import Combine
import Foundation

/// Client-side approval policy shown in the toolbar segmented control.
/// The Rust helper always runs in `--approval widget` mode; "Allow all" and
/// "Deny all" auto-answer incoming requests through the same audited command
/// path, so the 3-second undo window still applies.
public enum ApprovalPolicy: String, CaseIterable, Sendable, Hashable {
    case prompt
    case allowAll
    case denyAll

    public var title: String {
        switch self {
        case .prompt: "Prompt"
        case .allowAll: "Allow all"
        case .denyAll: "Deny all"
        }
    }
}

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
    case actRoom
}

/// Optional per-provider override. `defaultRule` follows the global arrival
/// strategy while the remaining cases pin that provider to a concrete policy.
public enum ForegroundAgentRule: String, CaseIterable, Codable, Sendable, Hashable {
    case defaultRule
    case immediate
    case remind
    case actRoom
}

/// Durable settings edited by the 台前调度 management page. Keeping the
/// policy in ActRealmKit gives the Runtime integration one authoritative
/// value instead of leaving behavior hidden inside view-local state.
public struct ForegroundSchedulingSettings: Codable, Equatable, Sendable {
    public var isEnabled: Bool
    public var closesAfterAcceptance: Bool
    public var strategy: ForegroundArrivalStrategy
    public var reminderSeconds: Int
    public var returnsToActRoom: Bool
    public var acceptanceSeconds: Int
    public var codexRule: ForegroundAgentRule
    public var claudeRule: ForegroundAgentRule

    public init(
        isEnabled: Bool = true,
        closesAfterAcceptance: Bool = true,
        strategy: ForegroundArrivalStrategy = .remind,
        reminderSeconds: Int = 10,
        returnsToActRoom: Bool = true,
        acceptanceSeconds: Int = 10,
        codexRule: ForegroundAgentRule = .defaultRule,
        claudeRule: ForegroundAgentRule = .immediate
    ) {
        self.isEnabled = isEnabled
        self.closesAfterAcceptance = closesAfterAcceptance
        self.strategy = strategy
        self.reminderSeconds = reminderSeconds
        self.returnsToActRoom = returnsToActRoom
        self.acceptanceSeconds = acceptanceSeconds
        self.codexRule = codexRule
        self.claudeRule = claudeRule
    }

    public static let defaults = ForegroundSchedulingSettings()
}

public enum ForegroundDispatchPhase: Equatable, Sendable {
    case reminding
    case opening
    case awaitingWorkspace
    case returnedToActRoom
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
    public var isActRoomReady: Bool
    public var isAgentAvailable: Bool
    public var isSchedulingWorkspaceActive: Bool

    public init(
        isActRoomReady: Bool = true,
        isAgentAvailable: Bool = true,
        isSchedulingWorkspaceActive: Bool = false
    ) {
        self.isActRoomReady = isActRoomReady
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

    @Published public var approvalPolicy: ApprovalPolicy {
        didSet { defaults.set(approvalPolicy.rawValue, forKey: Self.policyKey) }
    }

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

    public let isDemo: Bool
    public let supervisor: RuntimeSupervisor
    public let client: RuntimeClient

    private static let policyKey = "flowAgent.approvalPolicy"
    private static let dismissedTasksKey = "flowAgent.dismissedTaskVersions"
    private static let foregroundSchedulingKey = "flowAgent.foregroundScheduling"
    private let defaults: UserDefaults
    private var cancellables: Set<AnyCancellable> = []
    private var ticker: Task<Void, Never>?
    private var autoDecided: Set<String> = []
    private var dismissedTaskVersions: [String: UInt64]
    private var toastTask: Task<Void, Never>?
    private var outboxHighlightTask: Task<Void, Never>?
    private var hudPreviewTask: Task<Void, Never>?
    private var seededForegroundArrivals = false
    private var started = false

    public init(
        repoPath: URL? = nil,
        defaults: UserDefaults = .standard,
        demo: Bool = ProcessInfo.processInfo.environment["FLOW_DEMO"] == "1"
    ) {
        self.defaults = defaults
        self.isDemo = demo
        if !demo,
           let data = defaults.data(forKey: Self.foregroundSchedulingKey),
           let settings = try? JSONDecoder().decode(ForegroundSchedulingSettings.self, from: data)
        {
            self.foregroundScheduling = settings
        } else {
            self.foregroundScheduling = .defaults
        }
        self.foregroundDispatch = nil
        let supervisor = RuntimeSupervisor(repoPath: repoPath)
        self.supervisor = supervisor
        self.client = RuntimeClient()
        // The interaction redesign intentionally removed the Prompt / Allow
        // all / Deny all controls. Migrate any invisible legacy auto-policy
        // back to Prompt so a request can actually reach OUTBOX and the HUD.
        self.approvalPolicy = .prompt
        defaults.set(ApprovalPolicy.prompt.rawValue, forKey: Self.policyKey)
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
    }

    // MARK: - Lifecycle

    public func start() {
        guard !started else { return }
        started = true
        startTicker()
        if isDemo {
            derived = DemoData.derivedState(now: now)
            bridgeStatus = .listening
            lastSyncAt = now
            return
        }
        let supervisor = self.supervisor
        let client = self.client
        Task {
            supervisor.refreshDiagnostics()
            await supervisor.start { baseURL, token in
                Task { await client.connect(baseURL: baseURL, token: token) }
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
                Task { await client.connect(baseURL: baseURL, token: token) }
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

    private func startTicker() {
        ticker?.cancel()
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                self.now = Date()
                self.advanceForegroundDispatch(at: self.now)
                if self.isDemo {
                    self.derived = DemoData.derivedState(now: self.now)
                }
            }
        }
    }

    // MARK: - Snapshot handling

    private func apply(snapshot: Snapshot) {
        let next = DerivedState.derive(from: snapshot)
        let nextOpenIDs = Set(next.openOutbox.map(\.id))
        foregroundReturnNotes = foregroundReturnNotes.filter { nextOpenIDs.contains($0.key) }
        foregroundSuppressedAttentionIDs.formIntersection(nextOpenIDs)
        if let dispatch = foregroundDispatch, !nextOpenIDs.contains(dispatch.id) {
            foregroundDispatch = nil
        }

        if seededForegroundArrivals {
            let previousIDs = Set(derived.openOutbox.map(\.id))
            let arrivals = next.openOutbox
                .filter { !previousIDs.contains($0.id) }
                .sorted { $0.createdAt < $1.createdAt }
            for arrival in arrivals {
                receiveForegroundArrival(
                    id: arrival.id,
                    provider: arrival.provider,
                    title: arrival.actionTitle,
                    taskTitle: arrival.taskTitle
                )
            }
        } else {
            seededForegroundArrivals = true
        }

        derived = next
        lastSyncAt = Date()
        clampOutboxPage()
        autoDecideIfNeeded()
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

    /// Allow all / Deny all: answer fresh approval requests automatically,
    /// once per attention item, through the normal command path.
    private func autoDecideIfNeeded() {
        guard approvalPolicy != .prompt, !isDemo else { return }
        let action: AttentionAction = approvalPolicy == .allowAll ? .approve : .deny
        for entry in derived.openOutbox
        where entry.kind == .approval && entry.state == .open && !autoDecided.contains(entry.id) {
            autoDecided.insert(entry.id)
            let requestId = entry.attention.requestId
            Task { await client.send(action: action, attentionId: entry.id, requestId: requestId) }
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

    /// Clearing a row is a reversible presentation action, not deletion of
    /// local history. A newer event for the same session makes it visible
    /// again. Pending or running work cannot be hidden accidentally.
    public func dismissTask(_ task: LaneTask) {
        guard task.openOutboxCount == 0 else {
            revealOutbox(for: task)
            showToast("请先处理这个任务的 OUTBOX 事项")
            return
        }
        guard task.status != .running && task.status != .waiting else {
            showToast("正在运行的任务不能移除")
            return
        }
        dismissedTaskVersions[task.id] = task.session.lastEventAt
        defaults.set(
            dismissedTaskVersions.mapValues { NSNumber(value: $0) },
            forKey: Self.dismissedTasksKey
        )
        if expandedTaskId == task.id { expandedTaskId = nil }
        if pinnedSessionId == task.id { pinnedSessionId = nil }
        showToast("已从列表移除；有新活动时会自动恢复")
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

    private static func supportedSeconds(_ seconds: Int) -> Int {
        [5, 10, 30].min(by: { abs($0 - seconds) < abs($1 - seconds) }) ?? 10
    }

    public func effectiveForegroundStrategy(for provider: ProviderKind?) -> ForegroundArrivalStrategy {
        guard foregroundScheduling.isEnabled else { return .actRoom }
        let rule: ForegroundAgentRule
        switch provider {
        case .codex: rule = foregroundScheduling.codexRule
        case .claude: rule = foregroundScheduling.claudeRule
        case .gemini, nil: rule = .defaultRule
        }
        switch rule {
        case .defaultRule: return foregroundScheduling.strategy
        case .immediate: return .immediate
        case .remind: return .remind
        case .actRoom: return .actRoom
        }
    }

    public func openForegroundAgentNow() {
        guard let dispatch = foregroundDispatch else { return }
        beginOpening(dispatch, at: Date())
    }

    public func keepForegroundTaskInActRoom() {
        guard foregroundDispatch != nil else { return }
        foregroundDispatch = nil
        showToast("任务已留在 Act Room 待处理列表")
    }

    /// Deterministic visual preview used by SnapshotTool. It never creates a
    /// Runtime attention item or sends a provider command.
    public func previewForegroundScheduling() {
        guard isDemo else { return }
        foregroundDispatch = nil
        receiveForegroundArrival(
            id: "foreground-preview",
            provider: .codex,
            title: "Codex 想运行 Bash，等你批准",
            taskTitle: "升级依赖并修复构建脚本",
            at: now
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
        foregroundSuppressedAttentionIDs.insert(id)
        guard foregroundDispatch == nil else { return }

        switch effectiveForegroundStrategy(for: provider) {
        case .actRoom:
            showToast("新任务已进入 Act Room 待处理列表")
        case .immediate:
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
            guard foregroundScheduling.returnsToActRoom else {
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
                phase: .returnedToActRoom,
                startedAt: date,
                deadline: date.addingTimeInterval(1.4)
            )
            showToast(note)
        case .returnedToActRoom:
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
        hudPreviewTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(8))
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
