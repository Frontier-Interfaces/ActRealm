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

    /// High-caution approvals hide the direct Allow button: the primary
    /// action becomes "去原窗口核对" and Allow requires double confirmation.
    public var needsVerification: Bool {
        self == .high || self == .unknown
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

    public var id: String { attention.id }

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

    init(attention: AttentionRecord, sessionTitle: String?) {
        self.attention = attention
        self.kind = OutboxKind(record: attention.kind)
        self.risk = RiskLevel(record: attention.risk)
        self.state = OutboxItemState(record: attention.state)
        self.provider = ProviderKind(record: attention.provider)
        self.taskTitle = sessionTitle

        let providerName = ProviderKind(record: attention.provider)?.displayName ?? attention.provider
        var tool: String?
        if attention.title.hasPrefix("允许 ") {
            let stripped = attention.title.dropFirst("允许 ".count)
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

    public var id: String { session.id }

    public var title: String {
        if let title = session.providerTitle, !title.isEmpty { return title }
        if let title = session.title, !title.isEmpty { return title }
        if let project = session.project, !project.isEmpty { return project }
        return "未命名任务"
    }

    public var projectName: String? { session.project }
    public var model: String? { session.model }
    public var activity: String? { session.activity }
    public var activitySince: Date? { session.activitySince.map(ZhFormat.date(fromMillis:)) }
    public var lastEventAt: Date { ZhFormat.date(fromMillis: session.lastEventAt) }
    public var inputTokens: UInt64? { session.inputTokens }
    public var outputTokens: UInt64? { session.outputTokens }
    public var totalTokens: UInt64? { session.totalTokens }
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
        self.oldestOpenOutboxAt = openAttention.map(\.createdAt).min().map(ZhFormat.date(fromMillis:))
        self.firstOpenOutboxId = openAttention
            .min(by: { $0.createdAt < $1.createdAt })?.id
        self.primaryAttentionKind = openAttention
            .min(by: { $0.createdAt < $1.createdAt })
            .map { OutboxKind(record: $0.kind) }

        let waitingKinds: Set<String> = ["approval", "native_approval", "question"]
        let hasBlockingAttention = openAttention.contains { waitingKinds.contains($0.kind) }
        let hasCompletionAttention = openAttention.contains { $0.kind == "completion" }
        switch session.execState {
        case "awaiting_approval":
            self.status = .waiting
        case "thinking", "tool_running", "compacting":
            self.status = hasBlockingAttention ? .waiting : .running
        case "failed":
            self.status = .failed
        case "response_finished":
            self.status = (hasBlockingAttention || hasCompletionAttention) ? .waiting : .done
        default:
            self.status = (hasBlockingAttention || hasCompletionAttention) ? .waiting : .idle
        }
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
    public let source: String
    public let planType: String?
    public let windowMinutes: UInt64?
    public let availability: Availability

    public var id: String { slot.rawValue }

    /// Remaining < 20% → 紧张; nil when no trustworthy number exists.
    public var isTight: Bool {
        if case .available(let pct, _, _) = availability { return pct < 20 }
        return false
    }

    init(entry: QuotaEntry, index: Int) {
        self.slot = QuotaSlotID.make(entry: entry, index: index)
        self.title = Self.windowTitle(entry)
        self.source = entry.source
        self.planType = entry.planType
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
        case "stale":
            self.availability = .stale(
                remainingPct: remaining.map { max(0, min(100, $0)) },
                resetsAt: resetsAt,
                capturedAt: capturedAt
            )
        default:
            self.availability = .unavailable(reason: entry.reason)
        }
    }

    private static func windowTitle(_ entry: QuotaEntry) -> String {
        if let name = entry.limitName, !name.isEmpty { return name }
        if let minutes = entry.windowMinutes, minutes > 0 {
            if minutes.isMultiple(of: 43_200) { return "\(minutes / 43_200) 个月" }
            if minutes.isMultiple(of: 10_080) { return "\(minutes / 10_080) 周" }
            if minutes.isMultiple(of: 1_440) { return "\(minutes / 1_440) 天" }
            if minutes.isMultiple(of: 60) { return "\(minutes / 60) 小时" }
            return "\(minutes) 分钟"
        }
        switch entry.window {
        case "5h": return "5 小时"
        case "7d": return "7 天"
        case "week": return "本周"
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
        openOutbox.filter { $0.kind == .approval && $0.risk.needsVerification }.count
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

    /// Interaction-model task feed: one newest-first list across providers.
    public var agentTasks: [LaneTask] {
        lanes
            .flatMap(\.tasks)
            .sorted {
                if $0.lastEventAt != $1.lastEventAt { return $0.lastEventAt > $1.lastEventAt }
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
            .filter { visibleStates.contains($0.state) }
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
        where ["open", "committing", "decision_sent"].contains(item.state) {
            openAttentionBySession[item.sessionId, default: []].append(item)
        }

        // Render every validated Runtime window. M14 may add scoped model and
        // extra-usage windows, so the native client must not collapse them to
        // the three pre-M14 placeholders.
        let quotaSlots = snapshot.quota.enumerated()
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
