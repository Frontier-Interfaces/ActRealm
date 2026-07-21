import Foundation

// MARK: - Runtime snapshot

public struct SessionRecord: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let provider: String
    public let providerSessionId: String
    public let project: String?
    public let title: String?
    public let providerTitle: String?
    public let providerTitleSource: String?
    public let model: String?
    public let execState: String
    public let approvalOwner: String?
    public let activity: String?
    public let activitySince: UInt64?
    public let planDone: UInt32?
    public let planTotal: UInt32?
    public let turnStartedAt: UInt64?
    public let turnEndedAt: UInt64?
    /// Runtime field name. `totalTokens` remains as a compatibility alias for
    /// the older native UI and tests.
    public let tokenTotal: UInt64?
    public let contextWindowTokens: UInt64?
    public let inputTokens: UInt64?
    public let outputTokens: UInt64?
    public let cacheReadTokens: UInt64?
    public let cacheCreationTokens: UInt64?
    public let reasoningTokens: UInt64?
    public let lastTurnTokens: UInt64?
    public let contextUsedTokens: UInt64?
    public let contextUsedPercent: UInt32?
    public let estimatedCostUsdMicros: UInt64?
    public let costKind: String?
    public let pricingSource: String?
    public let usageSource: String?
    public let usageQuality: String?
    public let usageCapturedAt: UInt64?
    public let permissionMode: String?
    public let currentTool: String?
    public let activeSubagents: UInt32?
    public let providerTurnId: String?
    public let environment: String?
    public let jumpCapability: String?
    public let jumpLabel: String?
    public let controlCapability: String?
    public let recoveryState: String?
    public let canManage: Bool?
    public let connectorThreadStatus: String?
    public let lastEventAt: UInt64

    public var totalTokens: UInt64? { tokenTotal }

    public init(
        id: String,
        provider: String,
        providerSessionId: String,
        project: String?,
        title: String?,
        providerTitle: String? = nil,
        providerTitleSource: String? = nil,
        model: String?,
        execState: String,
        approvalOwner: String?,
        activity: String?,
        activitySince: UInt64?,
        planDone: UInt32?,
        planTotal: UInt32?,
        turnStartedAt: UInt64? = nil,
        turnEndedAt: UInt64? = nil,
        inputTokens: UInt64? = nil,
        outputTokens: UInt64? = nil,
        totalTokens: UInt64? = nil,
        tokenTotal: UInt64? = nil,
        contextWindowTokens: UInt64? = nil,
        cacheReadTokens: UInt64? = nil,
        cacheCreationTokens: UInt64? = nil,
        reasoningTokens: UInt64? = nil,
        lastTurnTokens: UInt64? = nil,
        contextUsedTokens: UInt64? = nil,
        contextUsedPercent: UInt32? = nil,
        estimatedCostUsdMicros: UInt64? = nil,
        costKind: String? = nil,
        pricingSource: String? = nil,
        usageSource: String? = nil,
        usageQuality: String? = nil,
        usageCapturedAt: UInt64? = nil,
        permissionMode: String? = nil,
        currentTool: String? = nil,
        activeSubagents: UInt32? = nil,
        providerTurnId: String? = nil,
        environment: String? = nil,
        jumpCapability: String? = nil,
        jumpLabel: String? = nil,
        controlCapability: String? = nil,
        recoveryState: String? = nil,
        canManage: Bool? = nil,
        connectorThreadStatus: String? = nil,
        lastEventAt: UInt64
    ) {
        self.id = id
        self.provider = provider
        self.providerSessionId = providerSessionId
        self.project = project
        self.title = title
        self.providerTitle = providerTitle
        self.providerTitleSource = providerTitleSource
        self.model = model
        self.execState = execState
        self.approvalOwner = approvalOwner
        self.activity = activity
        self.activitySince = activitySince
        self.planDone = planDone
        self.planTotal = planTotal
        self.turnStartedAt = turnStartedAt
        self.turnEndedAt = turnEndedAt
        self.tokenTotal = tokenTotal ?? totalTokens
        self.contextWindowTokens = contextWindowTokens
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheReadTokens = cacheReadTokens
        self.cacheCreationTokens = cacheCreationTokens
        self.reasoningTokens = reasoningTokens
        self.lastTurnTokens = lastTurnTokens
        self.contextUsedTokens = contextUsedTokens
        self.contextUsedPercent = contextUsedPercent
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.costKind = costKind
        self.pricingSource = pricingSource
        self.usageSource = usageSource
        self.usageQuality = usageQuality
        self.usageCapturedAt = usageCapturedAt
        self.permissionMode = permissionMode
        self.currentTool = currentTool
        self.activeSubagents = activeSubagents
        self.providerTurnId = providerTurnId
        self.environment = environment
        self.jumpCapability = jumpCapability
        self.jumpLabel = jumpLabel
        self.controlCapability = controlCapability
        self.recoveryState = recoveryState
        self.canManage = canManage
        self.connectorThreadStatus = connectorThreadStatus
        self.lastEventAt = lastEventAt
    }

    private enum CodingKeys: String, CodingKey {
        case id, provider, providerSessionId, project, title, providerTitle, providerTitleSource
        case model, execState, approvalOwner, activity, activitySince, planDone, planTotal
        case turnStartedAt, turnEndedAt, tokenTotal, contextWindowTokens
        case inputTokens, outputTokens, cacheReadTokens, cacheCreationTokens, reasoningTokens
        case lastTurnTokens, contextUsedTokens, contextUsedPercent, estimatedCostUsdMicros
        case costKind, pricingSource, usageSource, usageQuality, usageCapturedAt
        case permissionMode, currentTool, activeSubagents, providerTurnId, environment
        case jumpCapability, jumpLabel, controlCapability, recoveryState, canManage
        case connectorThreadStatus, lastEventAt
    }

    private enum LegacyCodingKeys: String, CodingKey {
        case totalTokens
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        id = try values.decode(String.self, forKey: .id)
        provider = try values.decode(String.self, forKey: .provider)
        providerSessionId = try values.decode(String.self, forKey: .providerSessionId)
        project = try values.decodeIfPresent(String.self, forKey: .project)
        title = try values.decodeIfPresent(String.self, forKey: .title)
        providerTitle = try values.decodeIfPresent(String.self, forKey: .providerTitle)
        providerTitleSource = try values.decodeIfPresent(String.self, forKey: .providerTitleSource)
        model = try values.decodeIfPresent(String.self, forKey: .model)
        execState = try values.decode(String.self, forKey: .execState)
        approvalOwner = try values.decodeIfPresent(String.self, forKey: .approvalOwner)
        activity = try values.decodeIfPresent(String.self, forKey: .activity)
        activitySince = try values.decodeIfPresent(UInt64.self, forKey: .activitySince)
        planDone = try values.decodeIfPresent(UInt32.self, forKey: .planDone)
        planTotal = try values.decodeIfPresent(UInt32.self, forKey: .planTotal)
        turnStartedAt = try values.decodeIfPresent(UInt64.self, forKey: .turnStartedAt)
        turnEndedAt = try values.decodeIfPresent(UInt64.self, forKey: .turnEndedAt)
        let legacy = try decoder.container(keyedBy: LegacyCodingKeys.self)
        tokenTotal = try values.decodeIfPresent(UInt64.self, forKey: .tokenTotal)
            ?? legacy.decodeIfPresent(UInt64.self, forKey: .totalTokens)
        contextWindowTokens = try values.decodeIfPresent(UInt64.self, forKey: .contextWindowTokens)
        inputTokens = try values.decodeIfPresent(UInt64.self, forKey: .inputTokens)
        outputTokens = try values.decodeIfPresent(UInt64.self, forKey: .outputTokens)
        cacheReadTokens = try values.decodeIfPresent(UInt64.self, forKey: .cacheReadTokens)
        cacheCreationTokens = try values.decodeIfPresent(UInt64.self, forKey: .cacheCreationTokens)
        reasoningTokens = try values.decodeIfPresent(UInt64.self, forKey: .reasoningTokens)
        lastTurnTokens = try values.decodeIfPresent(UInt64.self, forKey: .lastTurnTokens)
        contextUsedTokens = try values.decodeIfPresent(UInt64.self, forKey: .contextUsedTokens)
        contextUsedPercent = try values.decodeIfPresent(UInt32.self, forKey: .contextUsedPercent)
        estimatedCostUsdMicros = try values.decodeIfPresent(UInt64.self, forKey: .estimatedCostUsdMicros)
        costKind = try values.decodeIfPresent(String.self, forKey: .costKind)
        pricingSource = try values.decodeIfPresent(String.self, forKey: .pricingSource)
        usageSource = try values.decodeIfPresent(String.self, forKey: .usageSource)
        usageQuality = try values.decodeIfPresent(String.self, forKey: .usageQuality)
        usageCapturedAt = try values.decodeIfPresent(UInt64.self, forKey: .usageCapturedAt)
        permissionMode = try values.decodeIfPresent(String.self, forKey: .permissionMode)
        currentTool = try values.decodeIfPresent(String.self, forKey: .currentTool)
        activeSubagents = try values.decodeIfPresent(UInt32.self, forKey: .activeSubagents)
        providerTurnId = try values.decodeIfPresent(String.self, forKey: .providerTurnId)
        environment = try values.decodeIfPresent(String.self, forKey: .environment)
        jumpCapability = try values.decodeIfPresent(String.self, forKey: .jumpCapability)
        jumpLabel = try values.decodeIfPresent(String.self, forKey: .jumpLabel)
        controlCapability = try values.decodeIfPresent(String.self, forKey: .controlCapability)
        recoveryState = try values.decodeIfPresent(String.self, forKey: .recoveryState)
        canManage = try values.decodeIfPresent(Bool.self, forKey: .canManage)
        connectorThreadStatus = try values.decodeIfPresent(String.self, forKey: .connectorThreadStatus)
        lastEventAt = try values.decode(UInt64.self, forKey: .lastEventAt)
    }
}

public struct InteractiveOption: Codable, Equatable, Sendable, Identifiable {
    public let label: String
    public let description: String?
    public var id: String { label }
}

public struct InteractiveQuestion: Codable, Equatable, Sendable, Identifiable {
    public let id: String
    public let label: String
    public let prompt: String
    public let inputType: String
    public let multiSelect: Bool
    public let isSecret: Bool
    public let required: Bool
    public let allowsOther: Bool
    public let options: [InteractiveOption]
}

public struct InteractivePrompt: Codable, Equatable, Sendable {
    public let requestId: UUID
    public let kind: String
    public let provider: String
    public let title: String
    public let message: String?
    public let expiresAt: UInt64
    public let supportsNative: Bool
    public let questions: [InteractiveQuestion]
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
    public let interaction: InteractivePrompt?

    public init(
        id: String,
        sessionId: String,
        provider: String,
        project: String?,
        requestId: UUID?,
        kind: String,
        title: String,
        detail: String?,
        state: String,
        risk: String,
        riskNotes: [String],
        commandPreview: String?,
        expiresAt: UInt64?,
        createdAt: UInt64,
        resolution: String?,
        interaction: InteractivePrompt? = nil
    ) {
        self.id = id
        self.sessionId = sessionId
        self.provider = provider
        self.project = project
        self.requestId = requestId
        self.kind = kind
        self.title = title
        self.detail = detail
        self.state = state
        self.risk = risk
        self.riskNotes = riskNotes
        self.commandPreview = commandPreview
        self.expiresAt = expiresAt
        self.createdAt = createdAt
        self.resolution = resolution
        self.interaction = interaction
    }
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
    public let windowMinutes: UInt64?
    public let limitId: String?
    public let limitName: String?
    public let planType: String?
    public let capturedAt: UInt64?
    public let reason: String?

    public init(
        provider: String,
        window: String,
        status: String,
        usedPct: Double?,
        remainingPct: Double?,
        resetsAt: UInt64?,
        source: String,
        windowMinutes: UInt64? = nil,
        limitId: String? = nil,
        limitName: String? = nil,
        planType: String? = nil,
        capturedAt: UInt64?,
        reason: String?
    ) {
        self.provider = provider
        self.window = window
        self.status = status
        self.usedPct = usedPct
        self.remainingPct = remainingPct
        self.resetsAt = resetsAt
        self.source = source
        self.windowMinutes = windowMinutes
        self.limitId = limitId
        self.limitName = limitName
        self.planType = planType
        self.capturedAt = capturedAt
        self.reason = reason
    }
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

public struct CodexConnectorCapability: Codable, Equatable, Sendable {
    public let enabled: Bool?
    public let status: String?
    public let managedThreads: UInt64?
    public let error: String?
}

public struct SnapshotCapabilities: Codable, Equatable, Sendable {
    public let codexConnector: CodexConnectorCapability?
}

public struct Snapshot: Codable, Equatable, Sendable {
    public let sessions: [SessionRecord]
    public let attention: [AttentionRecord]
    public let commands: [CommandRecord]
    public let quota: [QuotaEntry]
    public let stats: SnapshotStats
    public let capabilities: SnapshotCapabilities?

    public init(
        sessions: [SessionRecord],
        attention: [AttentionRecord],
        commands: [CommandRecord],
        quota: [QuotaEntry],
        stats: SnapshotStats,
        capabilities: SnapshotCapabilities? = nil
    ) {
        self.sessions = sessions
        self.attention = attention
        self.commands = commands
        self.quota = quota
        self.stats = stats
        self.capabilities = capabilities
    }

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

// MARK: - Mutations

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

public enum JSONValue: Codable, Equatable, Sendable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case array([JSONValue])
    case object([String: JSONValue])
    case null

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() { self = .null }
        else if let value = try? container.decode(Bool.self) { self = .bool(value) }
        else if let value = try? container.decode(Double.self) { self = .number(value) }
        else if let value = try? container.decode(String.self) { self = .string(value) }
        else if let value = try? container.decode([JSONValue].self) { self = .array(value) }
        else { self = .object(try container.decode([String: JSONValue].self)) }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .bool(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        case .null: try container.encodeNil()
        }
    }
}

// MARK: - Agent setup

public struct SetupInfo: Codable, Equatable, Sendable {
    public struct ProviderSetup: Codable, Equatable, Sendable, Identifiable {
        public let provider: String
        public let status: String
        public let cliInstalled: Bool?
        public let desktopInstalled: Bool?
        public let desktopAppPath: String?
        public let reviewCommand: String?
        public let intent: String?
        public let configPath: String?
        public let ownedHandlers: UInt64?
        public let expectedHandlers: UInt64?
        public let binaryHealth: String?
        public let trustStatus: String?
        public let featureStatus: String?
        public let inlineEvents: [String]?
        public let canRepair: Bool?
        public let realEventVerified: Bool?

        public var id: String { provider }

        public var statusText: String {
            switch status {
            case "connected": "Hook 已安装 · 已验证真实事件"
            case "installed_unverified": "Hook 已安装 · 等待首个事件"
            case "needs_trust": "需要在 Codex 里完成信任确认（/hooks）"
            case "needs_reinstall": "Hook 需要重新安装"
            case "not_installed": "未安装 Hook"
            case "provider_missing", "cli_missing": "未找到该 Provider 的客户端"
            case "inline_conflict": "配置存在冲突，请检查"
            case "error": "配置读取出错"
            default: status
            }
        }

        public var isInstalled: Bool {
            ["connected", "installed_unverified", "needs_trust", "needs_reinstall"].contains(status)
        }

        public var detectedText: String {
            if cliInstalled == true && desktopInstalled == true { return "检测到桌面客户端与 CLI" }
            if desktopInstalled == true { return "检测到桌面客户端 · 不要求全局 CLI" }
            if cliInstalled == true { return "检测到 CLI" }
            return "尚未检测到可用客户端"
        }
    }

    public struct Safety: Codable, Equatable, Sendable {
        public let backsUpBeforeWrite: Bool
        public let codexTrustIsManual: Bool
        public let repairRespectsRemoval: Bool
    }

    public let schemaVersion: Int
    public let firstRun: Bool
    public let providers: [ProviderSetup]
    public let safety: Safety?
}

// MARK: - Local UI settings

public enum NotificationMode: String, Codable, CaseIterable, Equatable, Hashable, Sendable {
    case banner
    case list
    case ignore
}

public struct NotificationRules: Codable, Equatable, Sendable {
    public var approval: NotificationMode
    public var question: NotificationMode
    public var error: NotificationMode
    public var completion: NotificationMode

    public init(
        approval: NotificationMode = .list,
        question: NotificationMode = .list,
        error: NotificationMode = .list,
        completion: NotificationMode = .list
    ) {
        self.approval = approval
        self.question = question
        self.error = error
        self.completion = completion
    }

    public func mode(for kind: String) -> NotificationMode {
        switch kind {
        case "approval", "native_approval": approval
        case "question": question
        case "error": error
        case "completion": completion
        default: .list
        }
    }
}

public struct ProviderMuted: Codable, Equatable, Sendable {
    public var claude: Bool
    public var codex: Bool

    public init(claude: Bool = false, codex: Bool = false) {
        self.claude = claude
        self.codex = codex
    }

    public func contains(_ provider: String) -> Bool {
        provider == "claude" ? claude : provider == "codex" ? codex : false
    }
}

public struct UISettings: Codable, Equatable, Sendable {
    public var notificationRules: NotificationRules
    public var soundEnabled: Bool
    public var providerMuted: ProviderMuted
    public var codexEnhancedActivity: Bool
    public var retentionDays: UInt32
    public var displayProfile: String
    public var taskCardFields: [String]

    public init(
        notificationRules: NotificationRules = NotificationRules(),
        soundEnabled: Bool = true,
        providerMuted: ProviderMuted = ProviderMuted(),
        codexEnhancedActivity: Bool = true,
        retentionDays: UInt32 = 90,
        displayProfile: String = "detailed",
        taskCardFields: [String] = [
            "project", "task", "model", "activity", "plan", "tokens", "context",
            "tool", "subagents", "environment", "recovery", "control", "jump",
        ]
    ) {
        self.notificationRules = notificationRules
        self.soundEnabled = soundEnabled
        self.providerMuted = providerMuted
        self.codexEnhancedActivity = codexEnhancedActivity
        self.retentionDays = retentionDays
        self.displayProfile = displayProfile
        self.taskCardFields = taskCardFields
    }

    public static let defaults = UISettings()
}

public struct DisplayField: Codable, Equatable, Sendable, Identifiable {
    public let id: String
    public let label: String
    public let level: String
}

public struct ClaudeQuotaBridge: Codable, Equatable, Sendable {
    public let status: String
    public let configPath: String?
    public let helperPath: String?
    public let customConflict: Bool?
}

public struct SettingsResponse: Codable, Equatable, Sendable {
    public let settings: UISettings
    public let displayCatalog: [DisplayField]
    public let claudeQuotaBridge: ClaudeQuotaBridge
}

public enum AttentionAction: String, Sendable {
    case approve
    case deny
    case passThrough = "pass_through"
    case ack
    case snooze
    case dismiss
}
