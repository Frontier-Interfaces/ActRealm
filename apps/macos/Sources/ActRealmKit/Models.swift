import Foundation

public struct SessionRecord: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let provider: String
    public let providerSessionId: String
    public let project: String?
    public let title: String?
    public let model: String?
    public let execState: String
    public let approvalOwner: String?
    public let activity: String?
    public let activitySince: UInt64?
    public let planDone: UInt32?
    public let planTotal: UInt32?
    /// Provider-reported usage only. Nil means the provider did not expose a
    /// trustworthy value; the UI must never invent one.
    public let inputTokens: UInt64?
    public let outputTokens: UInt64?
    public let totalTokens: UInt64?
    public let contextWindowTokens: UInt64?
    public let usageCapturedAt: UInt64?
    public let lastEventAt: UInt64

    public init(
        id: String,
        provider: String,
        providerSessionId: String,
        project: String?,
        title: String?,
        model: String?,
        execState: String,
        approvalOwner: String?,
        activity: String?,
        activitySince: UInt64?,
        planDone: UInt32?,
        planTotal: UInt32?,
        inputTokens: UInt64? = nil,
        outputTokens: UInt64? = nil,
        totalTokens: UInt64? = nil,
        contextWindowTokens: UInt64? = nil,
        usageCapturedAt: UInt64? = nil,
        lastEventAt: UInt64
    ) {
        self.id = id
        self.provider = provider
        self.providerSessionId = providerSessionId
        self.project = project
        self.title = title
        self.model = model
        self.execState = execState
        self.approvalOwner = approvalOwner
        self.activity = activity
        self.activitySince = activitySince
        self.planDone = planDone
        self.planTotal = planTotal
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.totalTokens = totalTokens
        self.contextWindowTokens = contextWindowTokens
        self.usageCapturedAt = usageCapturedAt
        self.lastEventAt = lastEventAt
    }
}

public struct AttentionRecord: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let sessionId: String
    public let provider: String
    public let project: String?
    public let requestId: UUID?
    public let kind: String
    public let title: String
    public let detail: String?
    public let state: String
    public let risk: String
    public let riskNotes: [String]
    public let commandPreview: String?
    public let expiresAt: UInt64?
    public let createdAt: UInt64
    public let resolution: String?
}

public struct CommandRecord: Codable, Identifiable, Equatable, Sendable {
    public let id: UUID
    public let attentionId: String
    public let requestId: UUID?
    public let action: String
    public let state: String
    public let createdAt: UInt64
}

public struct QuotaEntry: Codable, Equatable, Sendable {
    public let provider: String
    public let window: String
    public let status: String
    public let usedPct: Double?
    public let remainingPct: Double?
    public let resetsAt: UInt64?
    public let source: String
    public let capturedAt: UInt64?
    public let reason: String?
}

public struct MetricsSummary: Codable, Equatable, Sendable {
    public let activeDays: UInt64
    public let approvalRequests: UInt64
    public let widgetApprovals: UInt64
    public let widgetDenials: UInt64
    public let passThroughManual: UInt64
    public let passThroughTimeout: UInt64
    public let decisionResponseMsTotal: UInt64
    public let decisionResponseCount: UInt64
    public let bannersShown: UInt64
    public let sessionsObserved: UInt64
    public let appOpened: UInt64
    public let todayWidgetDecisions: UInt64
}

public struct SnapshotStats: Codable, Equatable, Sendable {
    public let eventCount: UInt64
    public let metrics: MetricsSummary
}

public struct Snapshot: Codable, Equatable, Sendable {
    public let sessions: [SessionRecord]
    public let attention: [AttentionRecord]
    public let commands: [CommandRecord]
    public let quota: [QuotaEntry]
    public let stats: SnapshotStats

    public static let empty = Snapshot(
        sessions: [],
        attention: [],
        commands: [],
        quota: [],
        stats: SnapshotStats(eventCount: 0, metrics: MetricsSummary(
            activeDays: 0, approvalRequests: 0, widgetApprovals: 0, widgetDenials: 0,
            passThroughManual: 0, passThroughTimeout: 0, decisionResponseMsTotal: 0,
            decisionResponseCount: 0, bannersShown: 0, sessionsObserved: 0,
            appOpened: 0, todayWidgetDecisions: 0
        ))
    )
}

struct SnapshotEnvelope: Codable {
    let type: String
    let snapshot: Snapshot
}

public struct CommandRequest: Codable, Sendable {
    public let id: UUID
    public let attentionId: String
    public let requestId: UUID?
    public let action: String

    public init(id: UUID = UUID(), attentionId: String, requestId: UUID?, action: String) {
        self.id = id
        self.attentionId = attentionId
        self.requestId = requestId
        self.action = action
    }
}

public struct CommandResponse: Codable, Sendable {
    public let id: UUID
    public let state: String
}

public struct SetupInfo: Codable, Equatable, Sendable {
    public struct ProviderSetup: Codable, Equatable, Sendable {
        public let provider: String
        /// connected / needs_trust / installed_unverified / needs_reinstall /
        /// not_installed / provider_missing / inline_conflict / error
        public let status: String
        public let cliInstalled: Bool?
        public let desktopInstalled: Bool?
        public let canRepair: Bool?
        public let realEventVerified: Bool?

        public var statusText: String {
            switch status {
            case "connected": "Hook 已安装 · 已验证真实事件"
            case "installed_unverified": "Hook 已安装 · 等待首个事件"
            case "needs_trust": "需要在 Codex 里完成信任确认（/hooks）"
            case "needs_reinstall": "Hook 需要重新安装"
            case "not_installed": "未安装 Hook"
            case "provider_missing": "未找到该 Provider 的客户端"
            case "inline_conflict": "配置存在冲突，请检查"
            case "error": "配置读取出错"
            default: status
            }
        }

        public var isInstalled: Bool {
            ["connected", "installed_unverified", "needs_trust", "needs_reinstall"].contains(status)
        }
    }

    public let schemaVersion: Int
    public let firstRun: Bool
    public let providers: [ProviderSetup]
}

public enum AttentionAction: String, Sendable {
    case approve
    case deny
    case passThrough = "pass_through"
    case ack
    case snooze
}
