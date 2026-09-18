import Foundation

// MARK: - Provider identity

public enum ProviderKind: Sendable, Hashable {
    case claude
    case codex
    case gemini
    case custom(String)

    public init?(record value: String) {
        let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !normalized.isEmpty else { return nil }
        switch normalized {
        case "claude": self = .claude
        case "codex": self = .codex
        case "gemini": self = .gemini
        default: self = .custom(normalized)
        }
    }

    public var rawValue: String {
        switch self {
        case .claude: "claude"
        case .codex: "codex"
        case .gemini: "gemini"
        case let .custom(value): value
        }
    }

    public var displayName: String {
        switch self {
        case .claude: "Claude"
        case .codex: "Codex"
        case .gemini: "Gemini"
        case let .custom(value): value.split(separator: "-").map(\.capitalized).joined(separator: " ")
        }
    }

    /// Single-letter avatar badge used across the design ("C" / "X" / "G").
    public var avatarLetter: String {
        switch self {
        case .claude: "C"
        case .codex: "X"
        case .gemini: "G"
        case let .custom(value): String(value.prefix(1)).uppercased()
        }
    }

    /// "回复 ≤ 24h" — how long a hook waits before failing open.
    public var replyWindowText: String {
        switch self {
        case .claude: "回复 ≤ 24h"
        case .codex: "回复 ≤ 1h"
        case .gemini: "仅通知 · 200ms"
        case .custom: "由连接器能力决定"
        }
    }
}

// MARK: - Risk

public enum RiskLevel: String, Sendable {
    case low
    case med
    case high
    case unknown

    public init(record value: String) {
        self = RiskLevel(rawValue: value) ?? .unknown
    }

    public var badgeText: String {
        switch self {
        case .low: "低风险"
        case .med: "中风险"
        case .high: "高风险"
        case .unknown: "未识别"
        }
    }

    /// Emphasize the warning independently of the Runtime's reply capabilities.
    /// Risk classification never grants or removes an approval action.
    public var needsVerification: Bool {
        self != .low
    }
}

// MARK: - Outbox

public enum OutboxKind: String, Sendable {
    case approval
    case nativeApproval = "native_approval"
    case question
    case error
    case completion

    public init(record value: String) {
        self = OutboxKind(rawValue: value) ?? .question
    }

    public var badgeText: String {
        switch self {
        case .approval: "等待批准"
        case .nativeApproval: "原界面批准"
        case .question: "提问"
        case .error: "出错"
        case .completion: "完成"
        }
    }

    /// Spec §2.5: error/blocked first, then approvals, questions, completions.
    var sortRank: Int {
        switch self {
        case .error: 0
        case .approval, .nativeApproval: 1
        case .question: 2
        case .completion: 3
        }
    }
}

public enum OutboxItemState: String, Sendable {
    case open
    case committing
    case decisionSent = "decision_sent"
    case snoozed
    case resolved

    public init(record value: String) {
        self = OutboxItemState(rawValue: value) ?? .resolved
    }
}

public struct OutboxEntry: Identifiable, Equatable, Sendable {
    public let attention: AttentionRecord
    public let kind: OutboxKind
    public let risk: RiskLevel
    public let state: OutboxItemState
    public let provider: ProviderKind?
    /// Tool category parsed from the runtime title "允许 Bash？" → "Bash".
    public let toolName: String?
    /// Headline like "Codex 请求运行 Bash，等待批准".
    public let actionTitle: String
    /// Task title of the owning session, if known.
    public let taskTitle: String?
    public let createdAt: Date
    public let expiresAt: Date?
    public let autoHideAt: Date?

    public var id: String { attention.id }
    public var reminderAcknowledged: Bool { attention.reminderAcknowledgedAt != nil }

    /// Match Display: capabilities come from the live Runtime reply channel,
    /// not the tool name, command preview or risk label. An explicit empty
    /// list overrides legacy remoteActionable; unknown actions never expand it.
    public func approvalActions(at now: Date, connectionIsLive: Bool) -> Set<String> {
        guard connectionIsLive, kind == .approval, state == .open,
              attention.requestId != nil, let expiresAt, expiresAt > now
        else { return [] }
        if let declared = attention.allowedActions {
            return Set(declared).intersection(["approve", "deny"])
        }
        if attention.remoteActionable == true { return ["approve", "deny"] }
        return ["deny"]
    }

    /// "Codex · actrealm" source line.
    public var sourceLine: String {
        let name = provider?.displayName ?? attention.provider
        if let project = attention.project, !project.isEmpty {
            return "\(name) · \(project)"
        }
        return name
    }

    public var riskReason: String? {
        attention.riskNotes.first
    }

    public func localizedRiskReason(language: AppLanguage) -> String? {
        guard let fallback = attention.riskNotes.first else { return nil }
        return AppLocalization.localizedRuntimeMessage(
            attention.riskMessages?.first,
            fallback: fallback,
            language: language
        )
    }

    public func localizedDetail(language: AppLanguage) -> String? {
        guard let fallback = attention.detail else { return nil }
        return AppLocalization.localizedRuntimeMessage(
            attention.detailMessage,
            fallback: fallback,
            language: language
        )
    }

    public var actionTitleMessage: RuntimeMessage {
        if let titleMessage = attention.titleMessage {
            return titleMessage
        }
        let providerName = provider?.displayName ?? attention.provider
        return switch kind {
        case .approval:
            RuntimeMessage(code: "attention.approval.title")
        case .nativeApproval:
            RuntimeMessage(
                code: "attention.native_approval.title",
                args: ["provider": providerName]
            )
        case .question:
            RuntimeMessage(
                code: "attention.question.title",
                args: ["provider": providerName]
            )
        case .error:
            RuntimeMessage(code: "attention.error.title")
        case .completion:
            RuntimeMessage(code: "attention.completion.title")
        }
    }

    public func localizedActionTitle(language: AppLanguage) -> String {
        let providerName = provider?.displayName ?? attention.provider
        switch kind {
        case .approval:
            if let toolName {
                return AppLocalization.formatted(
                    "%@ 请求运行 %@，等待批准",
                    providerName,
                    toolName,
                    language: language
                )
            }
            return AppLocalization.formatted(
                "%@ 请求一次操作，等待批准",
                providerName,
                language: language
            )
        case .nativeApproval:
            return AppLocalization.localizedRuntimeMessage(
                actionTitleMessage,
                fallback: attention.title.isEmpty
                    ? "\(providerName) 等待在原界面批准"
                    : attention.title,
                language: language
            )
        case .question:
            return AppLocalization.localizedRuntimeMessage(
                actionTitleMessage,
                fallback: "\(providerName) 发出一个待回答问题",
                language: language
            )
        case .error:
            return AppLocalization.localizedRuntimeMessage(
                actionTitleMessage,
                fallback: "任务运行失败，需要检查",
                language: language
            )
        case .completion:
            return AppLocalization.localizedRuntimeMessage(
                actionTitleMessage,
                fallback: attention.title.isEmpty
                    ? "本轮修改已完成，等待确认。"
                    : attention.title,
                language: language
            )
        }
    }

    init(attention: AttentionRecord, sessionTitle: String?) {
        self.attention = attention
        self.kind = OutboxKind(record: attention.kind)
        self.risk = RiskLevel(record: attention.risk)
        self.state = OutboxItemState(record: attention.state)
        self.provider = ProviderKind(record: attention.provider)
        self.taskTitle = sessionTitle

        let providerName = ProviderKind(record: attention.provider)?.displayName ?? attention.provider
        var tool: String?
        if let prefix = ["允许 ", "Allow "].first(where: { attention.title.hasPrefix($0) }) {
            let stripped = attention.title.dropFirst(prefix.count)
            if let mark = stripped.firstIndex(where: { $0 == "？" || $0 == "?" }) {
                let candidate = String(stripped[..<mark]).trimmingCharacters(in: .whitespaces)
                if !candidate.isEmpty { tool = candidate }
            }
        }
        self.toolName = tool
        switch OutboxKind(record: attention.kind) {
        case .approval:
            if let tool {
                self.actionTitle = "\(providerName) 请求运行 \(tool)，等待批准"
            } else {
                self.actionTitle = "\(providerName) 请求一次操作，等待批准"
            }
        case .nativeApproval:
            self.actionTitle = attention.title.isEmpty
                ? "\(providerName) 等待在原界面批准"
                : attention.title
        case .question:
            self.actionTitle = "\(providerName) 发出一个待回答问题"
        case .error:
            self.actionTitle = "任务运行失败，需要检查"
        case .completion:
            self.actionTitle = "本轮修改已完成，等待确认"
        }
        self.createdAt = ZhFormat.date(fromMillis: attention.createdAt)
        self.expiresAt = attention.expiresAt.map(ZhFormat.date(fromMillis:))
        self.autoHideAt = attention.autoHideAt.map(ZhFormat.date(fromMillis:))
    }
}

// MARK: - Lane tasks

public enum LaneTaskStatus: Sendable, Equatable {
    case waiting
    case running
    case failed
    case done
    case idle

    public var badgeText: String {
        switch self {
        case .waiting: "等待"
        case .running: "在跑"
        case .failed: "出错"
        case .done: "完成"
        case .idle: "空闲"
        }
    }

    /// Stable tie-breaker after recency inside a lane.
    var sortRank: Int {
        switch self {
        case .waiting: 0
        case .running: 1
        case .failed: 2
        case .done: 3
        case .idle: 4
        }
    }
}

public enum RecoveryPresentation: Equatable, Sendable {
    case controllable
    case observing
    case waitingForEvent
    case lostControl
    case ended
    case unknown

    init(execState: String, recoveryState: String?) {
        if !Self.executionEnded(execState), recoveryState == "ended" {
            self = .unknown
            return
        }
        switch recoveryState {
        case "controllable": self = .controllable
        case "observing": self = .observing
        case "waiting_for_event": self = .waitingForEvent
        case "lost_control": self = .lostControl
        case "ended": self = .ended
        default: self = .unknown
        }
    }

    private static func executionEnded(_ execState: String) -> Bool {
        ["idle", "response_finished", "failed"].contains(execState)
    }
}

public struct TaskDetailRow: Equatable, Sendable {
    public let field: String
    public let value: String

    public init(field: String, value: String) {
        self.field = field
        self.value = value
    }
}

public struct LaneTask: Identifiable, Equatable, Sendable {
    public let session: SessionRecord
    public let status: LaneTaskStatus
    public let openOutboxCount: Int
    /// Includes snoozed presentation items so an older session remains in the
    /// recent task list without incorrectly looking blocked.
    public let hasVisibleAttention: Bool
    public let oldestOpenOutboxAt: Date?
    public let firstOpenOutboxId: String?
    public let primaryAttentionKind: OutboxKind?
    public let pendingInteractionState: String?
    public let awaitingCompletionConfirmation: Bool
    public let completionAcknowledged: Bool
    /// Product ordering: error/blocked, direct approval, Provider-native
    /// approval, question, running, completed, idle.
    public let taskPriorityRank: Int

    public var id: String { session.id }

    public var title: String {
        if let title = session.providerTitle, !title.isEmpty { return title }
        if let title = session.title, !title.isEmpty { return title }
        if let project = session.project, !project.isEmpty {
            return "\(providerFallbackName) · \(project)"
        }
        return "未命名任务"
    }

    private var providerFallbackName: String {
        switch session.provider {
        case "codex": "Codex"
        case "claude": "Claude"
        case "gemini": "Gemini"
        default: "Agent"
        }
    }

    public var projectName: String? { session.project }
    public var model: String? { session.model }
    public var activity: String? { session.activity }
    public var recoveryPresentation: RecoveryPresentation {
        RecoveryPresentation(execState: session.execState, recoveryState: session.recoveryState)
    }
    /// Fallback for a live execution with no finer-grained Runtime activity.
    public var activityLabel: String {
        status == .running ? "运行中" : status.badgeText
    }
    /// Facts that should disappear from the expanded card when Runtime did not
    /// provide them. A placeholder would turn missing Provider state into a
    /// claim about the Provider.
    public var detailRows: [TaskDetailRow] {
        var rows: [TaskDetailRow] = []
        if let currentTool = session.currentTool?.trimmingCharacters(in: .whitespacesAndNewlines),
           !currentTool.isEmpty {
            rows.append(TaskDetailRow(field: "tool", value: currentTool))
        }
        if let currentTarget = session.currentTarget?.trimmingCharacters(in: .whitespacesAndNewlines),
           !currentTarget.isEmpty {
            rows.append(TaskDetailRow(field: "target", value: currentTarget))
        }
        return rows
    }
    public func localizedState(language: AppLanguage) -> String {
        let key: String
        if status == .failed { key = "失败" }
        else if pendingInteractionState == "committing" { key = "正在提交决定" }
        else if pendingInteractionState == "decision_sent" { key = "等待 Agent 确认" }
        else if awaitingCompletionConfirmation { key = "完成待确认" }
        else if executionIsUnconfirmed { key = "等待新事件" }
        else {
            switch status {
            case .waiting: key = "等你处理"
            case .running: key = "运行中"
            case .failed: key = "失败"
            case .done: key = completionAcknowledged ? "已确认完成" : "本轮已完成"
            case .idle: key = "空闲"
            }
        }
        return AppLocalization.localized(key, language: language)
    }

    public func localizedCurrentAction(language: AppLanguage) -> String {
        let key: String
        if status == .failed { key = "任务失败" }
        else if pendingInteractionState == "committing" { key = "正在提交决定" }
        else if pendingInteractionState == "decision_sent" { key = "已处理，等待 Agent 确认" }
        else if awaitingCompletionConfirmation { key = "完成待确认" }
        else if status == .waiting {
            switch primaryAttentionKind {
            case .approval: key = "等待批准"
            case .nativeApproval: key = "等待原界面批准"
            case .question: key = "等待回答"
            case .completion: key = "完成待确认"
            default: key = "等待处理"
            }
        } else if executionIsUnconfirmed { key = "等待新事件" }
        else if status == .done { key = completionAcknowledged ? "已确认完成" : "本轮已完成" }
        else {
            switch session.execState {
            case "thinking": key = "正在思考"
            case "compacting": key = "正在整理上下文"
            case "tool_running":
                return TaskActivityPresentation.action(category: session.currentToolCategory,
                    tool: session.currentTool, running: true, language: language)
            default: key = "等待任务事件"
            }
        }
        return AppLocalization.localized(key, language: language)
    }

    public var activitySince: Date? { session.activitySince.map(ZhFormat.date(fromMillis:)) }
    public var lastEventAt: Date { ZhFormat.date(fromMillis: session.lastEventAt) }

    /// Active and waiting work never ages out. Once a task is inactive it
    /// leaves the compact Agent list immediately unless a current Attention
    /// item still needs to remain associated with it.
    public func isVisibleInAgentTasks(at _: Date) -> Bool {
        status == .running || status == .waiting || hasVisibleAttention
    }

    public var inputTokens: UInt64? { session.inputTokens }
    public var outputTokens: UInt64? { session.outputTokens }
    public var totalTokens: UInt64? { session.totalTokens }
    /// Per-task evidence remains useful while unrelated historical indexing is partial.
    public var usageIsProvisional: Bool {
        !["official", "official_local", "derived"].contains(session.usageQuality ?? "")
    }

    public var executionIsUnconfirmed: Bool {
        if session.execState == "waiting_for_event" { return true }
        return ["thinking", "tool_running", "compacting"].contains(session.execState)
            && ["lost_control", "waiting_for_event", "ended"].contains(session.recoveryState ?? "")
    }
    public var contextWindowTokens: UInt64? { session.contextWindowTokens }
    public var usageCapturedAt: Date? {
        session.usageCapturedAt.map(ZhFormat.date(fromMillis:))
    }

    /// A context percentage is shown only when both halves were reported by
    /// the provider. This intentionally stays nil for Claude transcripts that
    /// contain usage but no verifiable model window.
    public var contextUsageFraction: Double? {
        if let percent = session.contextUsedPercent {
            return max(0, min(1, Double(percent) / 100))
        }
        guard let used = session.contextUsedTokens,
              let window = contextWindowTokens,
              window > 0
        else { return nil }
        return max(0, min(1, Double(used) / Double(window)))
    }

    public var contextUsedTokens: UInt64? { session.contextUsedTokens }
    public var estimatedCostUsdMicros: UInt64? { session.estimatedCostUsdMicros }
    public var lastTurnTokens: UInt64? { session.lastTurnTokens }
    public var turnStartedAt: Date? { session.turnStartedAt.map(ZhFormat.date(fromMillis:)) }
    public var turnEndedAt: Date? { session.turnEndedAt.map(ZhFormat.date(fromMillis:)) }

    public func localizedTitle(language: AppLanguage) -> String {
        // Runtime status belongs in `activityMessage`. A task title may be
        // authored by the user or Provider and must remain verbatim even when
        // it happens to resemble a known status phrase.
        title
    }

    public func localizedActivity(language: AppLanguage) -> String? {
        guard let activity else { return nil }
        return AppLocalization.localizedRuntimeMessage(
            session.activityMessage,
            fallback: activity,
            language: language
        )
    }

    public var planProgress: (done: Int, total: Int)? {
        guard let done = session.planDone, let total = session.planTotal, total > 0 else { return nil }
        return (Int(done), Int(total))
    }

    init(
        session: SessionRecord,
        openAttention: [AttentionRecord],
        visibleAttention: [AttentionRecord]? = nil
    ) {
        self.session = session
        self.openOutboxCount = openAttention.count
        self.hasVisibleAttention = !(visibleAttention ?? openAttention).isEmpty
        func attentionRank(_ item: AttentionRecord) -> Int {
            switch item.kind {
            case "error": 0
            case "approval": 1
            case "native_approval": 2
            case "question", "elicitation": 3
            case "completion": 4
            default: 5
            }
        }
        let primary = openAttention.min {
            let left = attentionRank($0), right = attentionRank($1)
            return left == right ? $0.createdAt < $1.createdAt : left < right
        }
        self.oldestOpenOutboxAt = primary.map(\.createdAt).map(ZhFormat.date(fromMillis:))
        self.firstOpenOutboxId = primary?.id
        self.primaryAttentionKind = primary.map {
            OutboxKind(record: $0.kind == "elicitation" ? "question" : $0.kind)
        }
        let interactions = openAttention.filter {
            ["approval", "native_approval", "question", "elicitation"].contains($0.kind)
        }
        self.pendingInteractionState = interactions.contains(where: { $0.state == "open" }) ? nil
            : interactions.contains(where: { $0.state == "committing" }) ? "committing"
            : interactions.contains(where: { $0.state == "decision_sent" }) ? "decision_sent" : nil
        self.awaitingCompletionConfirmation = ["idle", "response_finished"].contains(session.execState)
            && interactions.isEmpty
            && !openAttention.contains { $0.kind == "error" }
            && openAttention.contains { $0.kind == "completion" && $0.reminderAcknowledgedAt == nil }
        self.completionAcknowledged = (visibleAttention ?? openAttention).contains {
            $0.kind == "completion" && $0.reminderAcknowledgedAt != nil
                && $0.createdAt >= (session.turnStartedAt ?? 0)
        }

        let waitingKinds: Set<String> = ["approval", "native_approval", "question", "elicitation"]
        let hasBlockingAttention = openAttention.contains { waitingKinds.contains($0.kind) }
        let hasCompletionAttention = openAttention.contains { $0.kind == "completion" }
        if session.execState == "failed" || openAttention.contains(where: { $0.kind == "error" }) {
            self.status = .failed
        } else {
        switch session.execState {
        case "awaiting_approval":
            self.status = .waiting
        case "thinking", "tool_running", "compacting":
            self.status = hasBlockingAttention ? .waiting : (
                ["lost_control", "waiting_for_event", "ended"].contains(session.recoveryState ?? "") ? .idle : .running
            )
        case "failed":
            self.status = .failed
        case "response_finished":
            self.status = (hasBlockingAttention || hasCompletionAttention) ? .waiting : .done
        default:
            self.status = (hasBlockingAttention || hasCompletionAttention) ? .waiting : .idle
        }
        }
        if session.execState == "failed" || openAttention.contains(where: { $0.kind == "error" }) {
            self.taskPriorityRank = 0
        } else if openAttention.contains(where: { $0.kind == "approval" }) {
            self.taskPriorityRank = 1
        } else if openAttention.contains(where: { $0.kind == "native_approval" }) {
            self.taskPriorityRank = 2
        } else if openAttention.contains(where: { $0.kind == "question" }) {
            self.taskPriorityRank = 3
        } else if self.status == .waiting {
            self.taskPriorityRank = 3
        } else if self.status == .running {
            self.taskPriorityRank = 4
        } else if hasCompletionAttention || self.status == .done {
            self.taskPriorityRank = 5
        } else {
            self.taskPriorityRank = 6
        }
    }
}

/// The immutable facts rendered by one task card. The identity is always the
/// Runtime Session ID, so presentation-only snapshot changes can be ignored
/// without giving a SwiftUI row a new value to render.
public struct TaskRenderSignature: Identifiable, Equatable, Sendable {
    public let sessionID: String
    public let task: LaneTask

    public var id: String { sessionID }

    public init(task: LaneTask) {
        self.sessionID = task.id
        self.task = task
    }
}

public struct TaskRenderProjectionResult: Equatable, Sendable {
    /// Ordered by the task feed, with each entry keyed by a stable Session ID.
    public let signatures: [TaskRenderSignature]
    /// Includes only cards with changed facts (or a removed Session ID).
    public let changedTaskIDs: [String]

    public init(signatures: [TaskRenderSignature], changedTaskIDs: [String]) {
        self.signatures = signatures
        self.changedTaskIDs = changedTaskIDs
    }
}

/// Retains the latest factual signature for each task card. It deliberately
/// projects only sessions plus their task-relevant attention: quota, metrics,
/// setup and clocks remain outside this cache.
public struct TaskRenderProjector: Sendable {
    private var signaturesBySessionID: [String: TaskRenderSignature] = [:]

    public init() {}

    public mutating func apply(_ snapshot: Snapshot) -> TaskRenderProjectionResult {
        apply(DerivedState.derive(from: snapshot).agentTasks)
    }

    public mutating func apply(_ tasks: [LaneTask]) -> TaskRenderProjectionResult {
        var nextBySessionID: [String: TaskRenderSignature] = [:]
        var orderedSessionIDs: [String] = []
        var changedTaskIDs: [String] = []

        for task in tasks {
            let signature = TaskRenderSignature(task: task)
            // Runtime Session IDs are unique. Keep the first occurrence
            // defensively rather than crashing if a malformed snapshot repeats
            // one, which preserves the same deterministic visible card.
            guard nextBySessionID[signature.sessionID] == nil else { continue }
            nextBySessionID[signature.sessionID] = signature
            orderedSessionIDs.append(signature.sessionID)
            if signaturesBySessionID[signature.sessionID] != signature {
                changedTaskIDs.append(signature.sessionID)
            }
        }

        let removedIDs = signaturesBySessionID.keys
            .filter { nextBySessionID[$0] == nil }
            .sorted()
        changedTaskIDs.append(contentsOf: removedIDs)
        signaturesBySessionID = nextBySessionID

        return TaskRenderProjectionResult(
            signatures: orderedSessionIDs.compactMap { nextBySessionID[$0] },
            changedTaskIDs: changedTaskIDs
        )
    }

    public static func diff(
        old: [LaneTask],
        new: [LaneTask]
    ) -> TaskRenderProjectionResult {
        var projector = TaskRenderProjector()
        _ = projector.apply(old)
        return projector.apply(new)
    }
}

public struct Lane: Identifiable, Equatable, Sendable {
    public let provider: ProviderKind
    public let tasks: [LaneTask]
    public let quotaSlots: [QuotaSlot]

    public var id: String { provider.rawValue }

    public var waitingCount: Int { tasks.filter { $0.status == .waiting }.count }
    public var runningCount: Int { tasks.filter { $0.status == .running }.count }

    /// Identity-column status dot: amber when someone waits on the user,
    /// green when work is active, gray when everything is idle.
    public enum Pulse: Sendable { case waiting, active, idle }
    public var pulse: Pulse {
        if waitingCount > 0 { return .waiting }
        if runningCount > 0 || tasks.contains(where: { $0.status == .done }) { return .active }
        return .idle
    }

    public var summaryLine: String {
        "\(tasks.count) 个任务 · \(provider.replyWindowText)"
    }
}

// MARK: - Quota

public struct QuotaSlotID: Hashable, Sendable {
    public let rawValue: String
    public let provider: ProviderKind

    public init(rawValue: String, provider: ProviderKind) {
        self.rawValue = rawValue
        self.provider = provider
    }

    public static let claude5h = QuotaSlotID(rawValue: "claude5h", provider: .claude)
    public static let claude7d = QuotaSlotID(rawValue: "claude7d", provider: .claude)
    public static let codexWeek = QuotaSlotID(rawValue: "codexWeek", provider: .codex)

    static func make(entry: QuotaEntry, index: Int) -> QuotaSlotID {
        switch (entry.provider, entry.window) {
        case ("claude", "5h"): .claude5h
        case ("claude", "7d"): .claude7d
        case ("codex", "week"): .codexWeek
        default:
            QuotaSlotID(
                rawValue: "\(entry.provider):\(entry.limitId ?? entry.window):\(index)",
                provider: ProviderKind(record: entry.provider) ?? .codex
            )
        }
    }
}

public struct QuotaSlot: Identifiable, Equatable, Sendable {
    public enum Availability: Equatable, Sendable {
        case available(remainingPct: Double, resetsAt: Date?, capturedAt: Date?)
        case stale(remainingPct: Double?, resetsAt: Date?, capturedAt: Date?)
        case unavailable(reason: String?)
    }

    public let slot: QuotaSlotID
    public let title: String
    public let titleMessage: RuntimeMessage?
    public let reasonMessage: RuntimeMessage?
    public let source: String
    public let resetSource: String?
    public let resetCapturedAt: Date?
    public let planType: String?
    public let isSpark: Bool
    public let windowMinutes: UInt64?
    public let availability: Availability

    public var id: String { slot.rawValue }

    public var providerDisplayName: String {
        if isSpark { return "Codex Spark" }
        return slot.provider.displayName
    }

    public var displayPlanType: String? {
        guard let planType else { return nil }
        let trimmed = planType.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    /// Remaining < 20% → 紧张; nil when no trustworthy number exists.
    public var isTight: Bool {
        if case .available(let pct, _, _) = availability { return pct < 20 }
        return false
    }

    public func localizedTitle(language: AppLanguage) -> String {
        AppLocalization.localizedRuntimeMessage(
            titleMessage,
            fallback: title,
            language: language
        )
    }

    public func localizedUnavailableReason(language: AppLanguage) -> String? {
        guard case .unavailable(let reason) = availability, let reason else { return nil }
        return AppLocalization.localizedRuntimeMessage(
            reasonMessage,
            fallback: reason,
            language: language
        )
    }

    init(entry: QuotaEntry, index: Int) {
        self.slot = QuotaSlotID.make(entry: entry, index: index)
        self.title = Self.windowTitle(entry)
        self.titleMessage = entry.windowMessage
        self.reasonMessage = entry.reasonMessage
        self.source = entry.source
        self.resetSource = entry.resetSource
        self.resetCapturedAt = entry.resetCapturedAt.map(ZhFormat.date(fromMillis:))
        self.planType = entry.planType
        self.isSpark = Self.isSparkEntry(entry)
        self.windowMinutes = entry.windowMinutes
        let remaining = entry.remainingPct ?? entry.usedPct.map { 100 - $0 }
        // Quota reset timestamps are epoch seconds; capture/event timestamps
        // elsewhere in the Runtime contract are milliseconds.
        let resetsAt = entry.resetsAt.map { Date(timeIntervalSince1970: TimeInterval($0)) }
        let capturedAt = entry.capturedAt.map(ZhFormat.date(fromMillis:))
        switch entry.status {
        case "available" where remaining != nil:
            self.availability = .available(
                remainingPct: max(0, min(100, remaining ?? 0)),
                resetsAt: resetsAt,
                capturedAt: capturedAt
            )
        case "stale" where remaining != nil:
            self.availability = .stale(
                remainingPct: remaining.map { max(0, min(100, $0)) },
                resetsAt: resetsAt,
                capturedAt: capturedAt
            )
        default:
            self.availability = .unavailable(reason: entry.reason)
        }
    }

    static func isSparkEntry(_ entry: QuotaEntry) -> Bool {
        entry.quotaKind == "spark"
    }

    static func isProPlan(_ planType: String?) -> Bool {
        planType?.trimmingCharacters(in: .whitespacesAndNewlines)
            .caseInsensitiveCompare("pro") == .orderedSame
    }

    private static func windowTitle(_ entry: QuotaEntry) -> String {
        if let name = entry.limitName, !name.isEmpty { return name }
        if let minutes = entry.windowMinutes, minutes > 0 {
            if (40_320 ... 44_640).contains(minutes) { return "1 month" }
            for (unitMinutes, unit) in [(UInt64(43_200), "month"), (10_080, "week"),
                                        (1_440, "day"), (60, "hour"), (1, "minute")] {
                if minutes.isMultiple(of: unitMinutes) {
                    let count = minutes / unitMinutes
                    return "\(count) \(unit)\(count == 1 ? "" : "s")"
                }
            }
        }
        switch entry.window {
        case "5h": return "5 hours"
        case "7d": return "7 days"
        case "week": return "This week"
        default: return entry.window.replacingOccurrences(of: "_", with: " ")
        }
    }
}

// MARK: - Pending decision (undo window)

public struct PendingDecision: Identifiable, Equatable, Sendable {
    public enum Phase: Equatable, Sendable {
        /// Inside the 3-second undo window.
        case undoable(deadline: Date)
        /// Sent to the provider, waiting for its confirming event.
        case sent
        /// Provider confirmed it kept going.
        case confirmed
    }

    public let commandId: UUID
    public let attentionID: String
    public let action: AttentionAction
    public let phase: Phase
    public let summary: String
    public let createdAt: Date

    public var id: UUID { commandId }

    public func localizedSummary(language: AppLanguage) -> String {
        AppLocalization.localizedProviderText(summary, language: language)
    }
}

// MARK: - Derived state

/// Pure projection of a runtime `Snapshot` into the shapes the Lanes+ UI
/// renders. Keeping it a free function makes ordering rules unit-testable.
public struct DerivedState: Equatable, Sendable {
    public static let undoWindow: TimeInterval = 3

    public let outbox: [OutboxEntry]
    public let lanes: [Lane]
    public let quotaSlots: [QuotaSlot]
    public let pendingDecision: PendingDecision?

    public var openOutbox: [OutboxEntry] {
        outbox.filter {
            $0.state == .open || $0.state == .committing
                || $0.state == .decisionSent
        }
    }

    public var highRiskOpenCount: Int {
        openOutbox.filter {
            $0.kind == .approval && ($0.risk == .high || $0.risk == .unknown)
        }.count
    }

    public var longestWait: TimeInterval? {
        guard let oldest = openOutbox.map(\.createdAt).min() else { return nil }
        return Date().timeIntervalSince(oldest)
    }

    public var totalTasks: Int { lanes.reduce(0) { $0 + $1.tasks.count } }
    public var waitingTasks: Int { lanes.reduce(0) { $0 + $1.waitingCount } }
    public var runningTasks: Int { lanes.reduce(0) { $0 + $1.runningCount } }
    public var doneTasks: Int {
        lanes.reduce(0) { $0 + $1.tasks.filter { $0.status == .done }.count }
    }

    /// Interaction-model task feed shared by the main window and menu bar.
    /// Routine tool events do not reorder running turns because their stable
    /// turn start, not their latest event, is the within-group key.
    public var agentTasks: [LaneTask] {
        lanes
            .flatMap(\.tasks)
            .sorted {
                if $0.taskPriorityRank != $1.taskPriorityRank {
                    return $0.taskPriorityRank < $1.taskPriorityRank
                }
                if $0.status == .waiting || $1.status == .waiting {
                    let lhsWait = $0.oldestOpenOutboxAt ?? $0.lastEventAt
                    let rhsWait = $1.oldestOpenOutboxAt ?? $1.lastEventAt
                    if lhsWait != rhsWait { return lhsWait < rhsWait }
                } else if $0.status == .running && $1.status == .running {
                    let lhsStart = $0.turnStartedAt ?? .distantPast
                    let rhsStart = $1.turnStartedAt ?? .distantPast
                    if lhsStart != rhsStart { return lhsStart > rhsStart }
                } else if $0.lastEventAt != $1.lastEventAt {
                    return $0.lastEventAt > $1.lastEventAt
                }
                return $0.id < $1.id
            }
    }

    public static let empty = DerivedState(outbox: [], lanes: [], quotaSlots: [], pendingDecision: nil)

    public init(
        outbox: [OutboxEntry],
        lanes: [Lane],
        quotaSlots: [QuotaSlot],
        pendingDecision: PendingDecision?
    ) {
        self.outbox = outbox
        self.lanes = lanes
        self.quotaSlots = quotaSlots
        self.pendingDecision = pendingDecision
    }

    public static func derive(from snapshot: Snapshot, now: Date = Date()) -> DerivedState {
        let sessionsById = Dictionary(uniqueKeysWithValues: snapshot.sessions.map { ($0.id, $0) })

        // OUTBOX: visible attention items, error > approval > question > done,
        // oldest waiting first inside a rank (spec §2.5).
        let visibleStates: Set<String> = ["open", "committing", "decision_sent", "snoozed"]
        let outbox = snapshot.attention
            .filter {
                visibleStates.contains($0.state)
                    && !($0.kind == "completion" && $0.reminderAcknowledgedAt != nil)
            }
            .map {
                let session = sessionsById[$0.sessionId]
                return OutboxEntry(
                    attention: $0,
                    sessionTitle: session?.providerTitle ?? session?.title
                )
            }
            .sorted {
                if $0.kind.sortRank != $1.kind.sortRank {
                    return $0.kind.sortRank < $1.kind.sortRank
                }
                return $0.createdAt < $1.createdAt
            }

        // Web keeps snoozed items associated with the session list but removes
        // them from its blocking/pending calculation until Runtime reopens
        // them. Preserve that distinction in the native projection.
        var visibleAttentionBySession: [String: [AttentionRecord]] = [:]
        for item in snapshot.attention
        where ["open", "committing", "decision_sent", "snoozed"].contains(item.state) {
            visibleAttentionBySession[item.sessionId, default: []].append(item)
        }

        // Pending Attention grouped per session for lane badges.
        var openAttentionBySession: [String: [AttentionRecord]] = [:]
        for item in snapshot.attention
        where ["open", "committing", "decision_sent"].contains(item.state)
            && !(item.kind == "completion" && item.reminderAcknowledgedAt != nil) {
            openAttentionBySession[item.sessionId, default: []].append(item)
        }

        // Render every validated Runtime window. M14 may add scoped model and
        // extra-usage windows, so the native client must not collapse them to
        // the three pre-M14 placeholders.
        let codexIsPro = snapshot.quota.contains {
            $0.provider == "codex" && QuotaSlot.isProPlan($0.planType)
        }
        let quotaSlots = snapshot.quota
            .filter { !QuotaSlot.isSparkEntry($0) || codexIsPro }
            .enumerated()
            .map { QuotaSlot(entry: $0.element, index: $0.offset) }

        // Claude and Codex lanes remain visible before their first task. Any
        // provider emitted by a future Runtime adapter receives its own lane
        // without requiring another native UI release.
        var tasksByProvider: [ProviderKind: [LaneTask]] = [:]
        for session in snapshot.sessions {
            guard let provider = ProviderKind(record: session.provider) else { continue }
            let task = LaneTask(
                session: session,
                openAttention: openAttentionBySession[session.id] ?? [],
                visibleAttention: visibleAttentionBySession[session.id] ?? []
            )
            tasksByProvider[provider, default: []].append(task)
        }
        for provider in tasksByProvider.keys {
            tasksByProvider[provider]?.sort {
                if $0.lastEventAt != $1.lastEventAt {
                    return $0.lastEventAt > $1.lastEventAt
                }
                if $0.status.sortRank != $1.status.sortRank {
                    return $0.status.sortRank < $1.status.sortRank
                }
                if $0.openOutboxCount != $1.openOutboxCount {
                    return $0.openOutboxCount > $1.openOutboxCount
                }
                return $0.id < $1.id
            }
        }
        var laneProviders: [ProviderKind] = [.claude, .codex]
        for provider in tasksByProvider.keys where !laneProviders.contains(provider) {
            laneProviders.append(provider)
        }
        let lanes = laneProviders
            .map { provider in
                Lane(
                    provider: provider,
                    tasks: tasksByProvider[provider] ?? [],
                    quotaSlots: quotaSlots.filter { $0.slot.provider == provider }
                )
            }
            .sorted { lhs, rhs in
                let lhsOldest = lhs.tasks
                    .filter { $0.status == .waiting }
                    .compactMap(\.oldestOpenOutboxAt).min() ?? .distantFuture
                let rhsOldest = rhs.tasks
                    .filter { $0.status == .waiting }
                    .compactMap(\.oldestOpenOutboxAt).min() ?? .distantFuture
                if lhsOldest != rhsOldest { return lhsOldest < rhsOldest }
                return lhs.provider.rawValue < rhs.provider.rawValue
            }

        // Undo capsule: newest command still inside (or just past) its window.
        let attentionById = Dictionary(uniqueKeysWithValues: snapshot.attention.map { ($0.id, $0) })
        let pending = snapshot.commands
            .compactMap { command -> PendingDecision? in
                let createdAt = ZhFormat.date(fromMillis: command.createdAt)
                let phase: PendingDecision.Phase
                switch command.state {
                case "pending_commit":
                    phase = .undoable(deadline: createdAt.addingTimeInterval(undoWindow))
                case "decision_sent" where now.timeIntervalSince(createdAt) < 12:
                    phase = .sent
                case "confirmed" where now.timeIntervalSince(createdAt) < 8:
                    phase = .confirmed
                default:
                    return nil
                }
                guard let action = AttentionAction(rawValue: command.action) else { return nil }
                let isUndoable: Bool
                if case .undoable = phase { isUndoable = true } else { isUndoable = false }
                let verb: String
                switch (action, isUndoable) {
                case (.approve, true): verb = "将允许"
                case (.deny, true): verb = "将拒绝"
                case (.passThrough, true): verb = "将交回"
                case (.approve, false): verb = "已允许"
                case (.deny, false): verb = "已拒绝"
                default: verb = "已交回"
                }
                var subject = "该操作"
                if let attention = attentionById[command.attentionId] {
                    let entry = OutboxEntry(
                        attention: attention,
                        sessionTitle: sessionsById[attention.sessionId]?.title
                    )
                    let providerName = entry.provider?.displayName ?? attention.provider
                    if let tool = entry.toolName {
                        let preview = attention.commandPreview.map { " \($0)" } ?? ""
                        subject = "\(providerName) 运行 \(tool)\(preview)"
                    } else {
                        subject = "\(providerName) 的请求"
                    }
                }
                return PendingDecision(
                    commandId: command.id,
                    attentionID: command.attentionId,
                    action: action,
                    phase: phase,
                    summary: "\(verb) \(subject)",
                    createdAt: createdAt
                )
            }
            .max(by: { $0.createdAt < $1.createdAt })

        return DerivedState(
            outbox: outbox,
            lanes: lanes,
            quotaSlots: quotaSlots,
            pendingDecision: pending
        )
    }
}

public enum TaskFactPresentation {
    public static func summary(
        _ fact: RuntimeFactMetadata,
        language: AppLanguage,
        now: Date = Date()
    ) -> String {
        let english = AppLanguage.resolvedIdentifier(selection: language)
            == AppLanguage.english.rawValue
        var parts = [sourceLabel(fact, english: english)]
        parts.append(freshnessLabel(fact.freshness, english: english))
        parts.append(verificationLabel(fact.verification, english: english))
        if let absenceReason = fact.absenceReason {
            parts.append(absenceLabel(absenceReason, english: english))
        }
        if fact.capability == .direct {
            parts.append(english ? "Direct action" : "可直接处理")
        } else if fact.capability == .returnToProvider {
            parts.append(english ? "Return to Provider" : "返回原应用")
        }
        if let capturedAt = fact.capturedAt {
            let captured = ZhFormat.date(fromMillis: capturedAt)
            parts.append(ZhFormat.relativeAgo(
                max(0, now.timeIntervalSince(captured)),
                language: language
            ))
        }
        return parts.joined(separator: " · ")
    }

    private static func sourceLabel(_ fact: RuntimeFactMetadata, english: Bool) -> String {
        guard let source = fact.sourceId else {
            return english ? "No source" : "无可用来源"
        }
        if source.hasPrefix("connector:") {
            return english ? "Provider connector" : "Provider Connector"
        }
        if source.hasPrefix("hook:") {
            return english ? "Provider hook" : "Provider Hook"
        }
        if source.hasPrefix("provider:") {
            return english ? "Provider event" : "Provider 事件"
        }
        if source.hasPrefix("runtime:") {
            return "Runtime"
        }
        return english ? "Bounded source" : "受限来源"
    }

    private static func freshnessLabel(_ value: RuntimeFactFreshness, english: Bool) -> String {
        switch value {
        case .live: english ? "Live" : "实时"
        case .delayed: english ? "Delayed" : "延迟"
        case .stale: english ? "Stale" : "已过期"
        case .expired: english ? "Expired" : "已失效"
        }
    }

    private static func verificationLabel(
        _ value: RuntimeFactVerification,
        english: Bool
    ) -> String {
        switch value {
        case .verified: english ? "Verified" : "已验证"
        case .partial: english ? "Partial" : "部分验证"
        case .unverified: english ? "Unverified" : "无法验证"
        case .notApplicable: english ? "Not applicable" : "不适用"
        }
    }

    private static func absenceLabel(
        _ value: RuntimeFactAbsenceReason,
        english: Bool
    ) -> String {
        switch value {
        case .providerNotSupplied:
            english ? "Provider did not supply it" : "Provider 未提供"
        case .notSupported:
            english ? "Provider does not support it" : "Provider 不支持"
        case .capabilityUnconfirmed:
            english ? "Capability is unconfirmed" : "能力尚未确认"
        case .noCurrentTurn:
            english ? "No current turn" : "当前 Turn 已结束"
        case .noCurrentActivity:
            english ? "No current activity" : "暂无当前活动"
        case .noCurrentTool:
            english ? "No current tool" : "当前阶段没有工具"
        case .currentToolHasNoTarget:
            english ? "Current tool has no file target" : "当前工具没有文件目标"
        case .taskNotCompleted:
            english ? "Task is not completed" : "任务尚未完成"
        case .sourceStale:
            english ? "Source is stale" : "来源已过期"
        case .unknown:
            english ? "Reason unavailable" : "原因不可用"
        }
    }
}
