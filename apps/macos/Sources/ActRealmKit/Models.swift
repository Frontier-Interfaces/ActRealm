import Foundation

// MARK: - Runtime snapshot

public struct RuntimeMessage: Codable, Equatable, Sendable {
    public let code: String
    public let args: [String: String]

    public init(code: String, args: [String: String] = [:]) {
        self.code = code
        self.args = args
    }
}

protocol RuntimeFactStringEnum: RawRepresentable, Codable where RawValue == String {
    static var fallback: Self { get }
}

extension RuntimeFactStringEnum {
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        self = Self(rawValue: (try? container.decode(String.self)) ?? "") ?? Self.fallback
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public enum RuntimeFactSourceKind: String, Codable, Equatable, Sendable,
    RuntimeFactStringEnum
{
    case authoritative
    case observed
    case derived
    case unavailable

    static let fallback = Self.unavailable
}

public enum RuntimeFactFreshness: String, Codable, Equatable, Sendable,
    RuntimeFactStringEnum
{
    case live
    case delayed
    case stale
    case expired

    static let fallback = Self.stale
}

public enum RuntimeFactVerification: String, Codable, Equatable, Sendable,
    RuntimeFactStringEnum
{
    case verified
    case partial
    case unverified
    case notApplicable = "not_applicable"

    static let fallback = Self.unverified
}

public enum RuntimeFactAbsenceReason: String, Codable, Equatable, Sendable,
    RuntimeFactStringEnum
{
    case providerNotSupplied = "provider_not_supplied"
    case notSupported = "not_supported"
    case capabilityUnconfirmed = "capability_unconfirmed"
    case noCurrentTurn = "no_current_turn"
    case noCurrentActivity = "no_current_activity"
    case noCurrentTool = "no_current_tool"
    case currentToolHasNoTarget = "current_tool_has_no_target"
    case taskNotCompleted = "task_not_completed"
    case sourceStale = "source_stale"
    case unknown

    static let fallback = Self.unknown
}

public enum RuntimeFactCapability: String, Codable, Equatable, Sendable,
    RuntimeFactStringEnum
{
    case direct
    case returnToProvider = "return_to_provider"
    case observeOnly = "observe_only"
    case unavailable

    static let fallback = Self.unavailable
}

public struct RuntimeFactMetadata: Codable, Equatable, Sendable {
    public static let supportedSchemaVersion = 1

    public let schemaVersion: Int
    public let sourceKind: RuntimeFactSourceKind
    public let sourceId: String?
    public let capturedAt: UInt64?
    public let freshness: RuntimeFactFreshness
    public let verification: RuntimeFactVerification
    public let absenceReason: RuntimeFactAbsenceReason?
    public let capability: RuntimeFactCapability

    public init(
        schemaVersion: Int = Self.supportedSchemaVersion,
        sourceKind: RuntimeFactSourceKind,
        sourceId: String? = nil,
        capturedAt: UInt64? = nil,
        freshness: RuntimeFactFreshness,
        verification: RuntimeFactVerification,
        absenceReason: RuntimeFactAbsenceReason? = nil,
        capability: RuntimeFactCapability
    ) {
        self.schemaVersion = schemaVersion
        self.sourceKind = schemaVersion == Self.supportedSchemaVersion ? sourceKind : .unavailable
        self.sourceId = schemaVersion == Self.supportedSchemaVersion
            ? Self.boundedSourceID(sourceId) : nil
        self.capturedAt = schemaVersion == Self.supportedSchemaVersion ? capturedAt : nil
        self.freshness = schemaVersion == Self.supportedSchemaVersion ? freshness : .stale
        self.verification = schemaVersion == Self.supportedSchemaVersion
            ? verification : .unverified
        self.absenceReason = schemaVersion == Self.supportedSchemaVersion
            ? absenceReason : .capabilityUnconfirmed
        self.capability = schemaVersion == Self.supportedSchemaVersion
            ? capability : .unavailable
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, sourceKind, sourceId, capturedAt, freshness
        case verification, absenceReason, capability
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            schemaVersion: (try? container.decode(Int.self, forKey: .schemaVersion)) ?? 0,
            sourceKind: (try? container.decode(
                RuntimeFactSourceKind.self,
                forKey: .sourceKind
            )) ?? .unavailable,
            sourceId: try? container.decodeIfPresent(String.self, forKey: .sourceId),
            capturedAt: try? container.decodeIfPresent(UInt64.self, forKey: .capturedAt),
            freshness: (try? container.decode(
                RuntimeFactFreshness.self,
                forKey: .freshness
            )) ?? .stale,
            verification: (try? container.decode(
                RuntimeFactVerification.self,
                forKey: .verification
            )) ?? .unverified,
            absenceReason: try? container.decodeIfPresent(
                RuntimeFactAbsenceReason.self,
                forKey: .absenceReason
            ),
            capability: (try? container.decode(
                RuntimeFactCapability.self,
                forKey: .capability
            )) ?? .unavailable
        )
    }

    private static func boundedSourceID(_ value: String?) -> String? {
        guard let value,
              !value.isEmpty,
              value.utf8.count <= 160,
              let colon = value.firstIndex(of: ":"),
              colon != value.startIndex
        else { return nil }
        let prefix = value[..<colon]
        let suffix = value[value.index(after: colon)...]
        let prefixBytes = Array(prefix.utf8)
        let suffixBytes = Array(suffix.utf8)
        guard let first = prefixBytes.first,
              (UInt8(ascii: "a")...UInt8(ascii: "z")).contains(first),
              !suffixBytes.isEmpty,
              prefixBytes.allSatisfy({ byte in
                  (UInt8(ascii: "a")...UInt8(ascii: "z")).contains(byte)
                      || (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(byte)
                      || [UInt8(ascii: "."), UInt8(ascii: "_"), UInt8(ascii: "-")].contains(byte)
              }),
              suffixBytes.allSatisfy({ byte in
                  (UInt8(ascii: "a")...UInt8(ascii: "z")).contains(byte)
                      || (UInt8(ascii: "A")...UInt8(ascii: "Z")).contains(byte)
                      || (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(byte)
                      || [
                          UInt8(ascii: "."), UInt8(ascii: "_"), UInt8(ascii: "-"),
                          UInt8(ascii: "/")
                      ].contains(byte)
              })
        else { return nil }
        return value
    }

    public static let unavailable = RuntimeFactMetadata(
        schemaVersion: Self.supportedSchemaVersion,
        sourceKind: .unavailable,
        freshness: .stale,
        verification: .unverified,
        absenceReason: .capabilityUnconfirmed,
        capability: .unavailable
    )
}

public struct RuntimeSessionFacts: Codable, Equatable, Sendable {
    public static let supportedSchemaVersion = 1

    public let schemaVersion: Int
    public let plan: RuntimeFactMetadata
    public let activity: RuntimeFactMetadata
    public let currentTarget: RuntimeFactMetadata
    public let completion: RuntimeFactMetadata
    public let control: RuntimeFactMetadata

    public init(
        schemaVersion: Int = Self.supportedSchemaVersion,
        plan: RuntimeFactMetadata,
        activity: RuntimeFactMetadata,
        currentTarget: RuntimeFactMetadata,
        completion: RuntimeFactMetadata,
        control: RuntimeFactMetadata
    ) {
        self.schemaVersion = schemaVersion
        if schemaVersion == Self.supportedSchemaVersion {
            self.plan = plan
            self.activity = activity
            self.currentTarget = currentTarget
            self.completion = completion
            self.control = control
        } else {
            self.plan = .unavailable
            self.activity = .unavailable
            self.currentTarget = .unavailable
            self.completion = .unavailable
            self.control = .unavailable
        }
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, plan, activity, currentTarget, completion, control
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.init(
            schemaVersion: (try? container.decode(Int.self, forKey: .schemaVersion)) ?? 0,
            plan: (try? container.decode(RuntimeFactMetadata.self, forKey: .plan)) ?? .unavailable,
            activity: (try? container.decode(
                RuntimeFactMetadata.self,
                forKey: .activity
            )) ?? .unavailable,
            currentTarget: (try? container.decode(
                RuntimeFactMetadata.self,
                forKey: .currentTarget
            )) ?? .unavailable,
            completion: (try? container.decode(
                RuntimeFactMetadata.self,
                forKey: .completion
            )) ?? .unavailable,
            control: (try? container.decode(
                RuntimeFactMetadata.self,
                forKey: .control
            )) ?? .unavailable
        )
    }
}

public struct PlanStepRecord: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let text: String
    public let detail: String?
    public let status: String
    public let source: String

    public init(id: String, text: String, detail: String?, status: String, source: String) {
        self.id = id
        self.text = text
        self.detail = detail
        self.status = status
        self.source = source
    }
}

public struct SubagentRecord: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let agentType: String?
    public let status: String
    public let source: String?

    public init(id: String, agentType: String?, status: String, source: String?) {
        self.id = id
        self.agentType = agentType
        self.status = status
        self.source = source
    }
}

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
    public let activityMessage: RuntimeMessage?
    public let activitySince: UInt64?
    public let planDone: UInt32?
    public let planTotal: UInt32?
    public let planSteps: [PlanStepRecord]
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
    public let currentToolCategory: String?
    public let currentTarget: String?
    public let activeSubagents: UInt32?
    public let subagents: [SubagentRecord]
    public let providerTurnId: String?
    public let environment: String?
    public let jumpCapability: String?
    public let jumpLabel: String?
    public let jumpMessage: RuntimeMessage?
    public let controlCapability: String?
    public let recoveryState: String?
    public let canManage: Bool?
    public let connectorThreadStatus: String?
    public let facts: RuntimeSessionFacts?
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
        activityMessage: RuntimeMessage? = nil,
        activitySince: UInt64?,
        planDone: UInt32?,
        planTotal: UInt32?,
        planSteps: [PlanStepRecord] = [],
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
        currentToolCategory: String? = nil,
        currentTarget: String? = nil,
        activeSubagents: UInt32? = nil,
        subagents: [SubagentRecord] = [],
        providerTurnId: String? = nil,
        environment: String? = nil,
        jumpCapability: String? = nil,
        jumpLabel: String? = nil,
        jumpMessage: RuntimeMessage? = nil,
        controlCapability: String? = nil,
        recoveryState: String? = nil,
        canManage: Bool? = nil,
        connectorThreadStatus: String? = nil,
        facts: RuntimeSessionFacts? = nil,
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
        self.activityMessage = activityMessage
        self.activitySince = activitySince
        self.planDone = planDone
        self.planTotal = planTotal
        self.planSteps = planSteps
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
        self.currentToolCategory = currentToolCategory
        self.currentTarget = currentTarget
        self.activeSubagents = activeSubagents
        self.subagents = subagents
        self.providerTurnId = providerTurnId
        self.environment = environment
        self.jumpCapability = jumpCapability
        self.jumpLabel = jumpLabel
        self.jumpMessage = jumpMessage
        self.controlCapability = controlCapability
        self.recoveryState = recoveryState
        self.canManage = canManage
        self.connectorThreadStatus = connectorThreadStatus
        self.facts = facts
        self.lastEventAt = lastEventAt
    }

    private enum CodingKeys: String, CodingKey {
        case id, provider, providerSessionId, project, title, providerTitle, providerTitleSource
        case model, execState, approvalOwner, activity, activityMessage, activitySince
        case planDone, planTotal, planSteps
        case turnStartedAt, turnEndedAt, tokenTotal, contextWindowTokens
        case inputTokens, outputTokens, cacheReadTokens, cacheCreationTokens, reasoningTokens
        case lastTurnTokens, contextUsedTokens, contextUsedPercent, estimatedCostUsdMicros
        case costKind, pricingSource, usageSource, usageQuality, usageCapturedAt
        case permissionMode, currentTool, currentToolCategory, currentTarget
        case activeSubagents, subagents, providerTurnId, environment
        case jumpCapability, jumpLabel, jumpMessage, controlCapability, recoveryState, canManage
        case connectorThreadStatus, facts, lastEventAt
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
        activityMessage = try values.decodeIfPresent(RuntimeMessage.self, forKey: .activityMessage)
        activitySince = try values.decodeIfPresent(UInt64.self, forKey: .activitySince)
        planDone = try values.decodeIfPresent(UInt32.self, forKey: .planDone)
        planTotal = try values.decodeIfPresent(UInt32.self, forKey: .planTotal)
        planSteps = try values.decodeIfPresent([PlanStepRecord].self, forKey: .planSteps) ?? []
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
        currentToolCategory = try values.decodeIfPresent(String.self, forKey: .currentToolCategory)
        currentTarget = try values.decodeIfPresent(String.self, forKey: .currentTarget)
        activeSubagents = try values.decodeIfPresent(UInt32.self, forKey: .activeSubagents)
        subagents = try values.decodeIfPresent([SubagentRecord].self, forKey: .subagents) ?? []
        providerTurnId = try values.decodeIfPresent(String.self, forKey: .providerTurnId)
        environment = try values.decodeIfPresent(String.self, forKey: .environment)
        jumpCapability = try values.decodeIfPresent(String.self, forKey: .jumpCapability)
        jumpLabel = try values.decodeIfPresent(String.self, forKey: .jumpLabel)
        jumpMessage = try values.decodeIfPresent(RuntimeMessage.self, forKey: .jumpMessage)
        controlCapability = try values.decodeIfPresent(String.self, forKey: .controlCapability)
        recoveryState = try values.decodeIfPresent(String.self, forKey: .recoveryState)
        canManage = try values.decodeIfPresent(Bool.self, forKey: .canManage)
        connectorThreadStatus = try values.decodeIfPresent(String.self, forKey: .connectorThreadStatus)
        facts = try? values.decode(RuntimeSessionFacts.self, forKey: .facts)
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
    public let titleCode: String?
    public let message: String?
    public let expiresAt: UInt64
    public let supportsNative: Bool
    public let questions: [InteractiveQuestion]

    public init(
        requestId: UUID,
        kind: String,
        provider: String,
        title: String,
        titleCode: String? = nil,
        message: String?,
        expiresAt: UInt64,
        supportsNative: Bool,
        questions: [InteractiveQuestion]
    ) {
        self.requestId = requestId
        self.kind = kind
        self.provider = provider
        self.title = title
        self.titleCode = titleCode
        self.message = message
        self.expiresAt = expiresAt
        self.supportsNative = supportsNative
        self.questions = questions
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
    public let titleMessage: RuntimeMessage?
    public let detail: String?
    public let detailMessage: RuntimeMessage?
    public let state: String
    public let risk: String
    public let riskNotes: [String]
    public let riskMessages: [RuntimeMessage]?
    public let primaryCategory: String?
    public let riskCodes: [String]?
    public let commandPreview: String?
    public let expiresAt: UInt64?
    public let autoHideAt: UInt64?
    public let reminderAcknowledgedAt: UInt64?
    public let reminderResolution: String?
    public let createdAt: UInt64
    public let resolution: String?
    public let interaction: InteractivePrompt?
    public let remoteActionable: Bool?
    public let allowedActions: [String]?

    public init(
        id: String,
        sessionId: String,
        provider: String,
        project: String?,
        requestId: UUID?,
        kind: String,
        title: String,
        titleMessage: RuntimeMessage? = nil,
        detail: String?,
        detailMessage: RuntimeMessage? = nil,
        state: String,
        risk: String,
        riskNotes: [String],
        riskMessages: [RuntimeMessage]? = nil,
        primaryCategory: String? = nil,
        riskCodes: [String]? = nil,
        commandPreview: String?,
        expiresAt: UInt64?,
        autoHideAt: UInt64? = nil,
        reminderAcknowledgedAt: UInt64? = nil,
        reminderResolution: String? = nil,
        createdAt: UInt64,
        resolution: String?,
        interaction: InteractivePrompt? = nil,
        remoteActionable: Bool? = nil,
        allowedActions: [String]? = nil
    ) {
        self.id = id
        self.sessionId = sessionId
        self.provider = provider
        self.project = project
        self.requestId = requestId
        self.kind = kind
        self.title = title
        self.titleMessage = titleMessage
        self.detail = detail
        self.detailMessage = detailMessage
        self.state = state
        self.risk = risk
        self.riskNotes = riskNotes
        self.riskMessages = riskMessages
        self.primaryCategory = primaryCategory
        self.riskCodes = riskCodes
        self.commandPreview = commandPreview
        self.expiresAt = expiresAt
        self.autoHideAt = autoHideAt
        self.reminderAcknowledgedAt = reminderAcknowledgedAt
        self.reminderResolution = reminderResolution
        self.createdAt = createdAt
        self.resolution = resolution
        self.interaction = interaction
        self.remoteActionable = remoteActionable
        self.allowedActions = allowedActions
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
    public let resetSource: String?
    public let resetCapturedAt: UInt64?
    public let source: String
    public let windowMinutes: UInt64?
    public let limitId: String?
    public let limitName: String?
    public let quotaKind: String?
    public let planType: String?
    public let capturedAt: UInt64?
    public let reason: String?
    public let reasonCode: String?
    public let reasonArgs: [String: String]?
    public let windowMessage: RuntimeMessage?
    public let reasonMessage: RuntimeMessage?

    public init(
        provider: String,
        window: String,
        status: String,
        usedPct: Double?,
        remainingPct: Double?,
        resetsAt: UInt64?,
        resetSource: String? = nil,
        resetCapturedAt: UInt64? = nil,
        source: String,
        windowMinutes: UInt64? = nil,
        limitId: String? = nil,
        limitName: String? = nil,
        quotaKind: String? = nil,
        planType: String? = nil,
        capturedAt: UInt64?,
        reason: String?,
        reasonCode: String? = nil,
        reasonArgs: [String: String]? = nil,
        windowMessage: RuntimeMessage? = nil,
        reasonMessage: RuntimeMessage? = nil
    ) {
        self.provider = provider
        self.window = window
        self.status = status
        self.usedPct = usedPct
        self.remainingPct = remainingPct
        self.resetsAt = resetsAt
        self.resetSource = resetSource
        self.resetCapturedAt = resetCapturedAt
        self.source = source
        self.windowMinutes = windowMinutes
        self.limitId = limitId
        self.limitName = limitName
        self.quotaKind = quotaKind
        self.planType = planType
        self.capturedAt = capturedAt
        self.reason = reason
        self.reasonCode = reasonCode
        self.reasonArgs = reasonArgs
        self.windowMessage = windowMessage
        self.reasonMessage = reasonMessage
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
    public let managedApprovals: Bool?
    public let serverUserAgent: String?
    public let lastNotificationMethod: String?
    public let lastNotificationAt: UInt64?
    public let lastPlanSkipReason: String?
    public let lastPlanFieldKeys: [String]?
}

public enum ProviderCapabilityStatus: String, Codable, Equatable, Sendable {
    case supported
    case unsupported
    case unknown

    public var canClaimSupport: Bool {
        self == .supported
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let value = try? container.decode(String.self)
        self = value.flatMap(Self.init(rawValue:)) ?? .unknown
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public enum ProviderCapabilityFeature: String, CaseIterable, Equatable, Sendable {
    case plan
    case subagents
    case approvals
    case transcriptSlice
    case toolLifecycle
    case currentTarget
}

public struct ProviderCapabilityFact: Codable, Equatable, Sendable {
    public let status: ProviderCapabilityStatus
    public let source: String?

    public static let unknown = ProviderCapabilityFact(status: .unknown)

    public init(status: ProviderCapabilityStatus, source: String? = nil) {
        if status == .supported,
           let source,
           Self.validSource(source) {
            self.status = .supported
            self.source = source
        } else {
            self.status = status == .supported ? .unknown : status
            self.source = nil
        }
    }

    private enum CodingKeys: String, CodingKey {
        case status, source
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let status = (try? container.decode(
            ProviderCapabilityStatus.self,
            forKey: .status
        )) ?? .unknown
        let source = try? container.decode(String.self, forKey: .source)
        self.init(status: status, source: source)
    }

    private static func validSource(_ source: String) -> Bool {
        !source.isEmpty
            && source.utf8.count <= 160
            && source.unicodeScalars.allSatisfy {
                !CharacterSet.controlCharacters.contains($0)
            }
    }
}

public struct ProviderCapabilitySet: Codable, Equatable, Sendable {
    public let plan: ProviderCapabilityFact
    public let subagents: ProviderCapabilityFact
    public let approvals: ProviderCapabilityFact
    public let transcriptSlice: ProviderCapabilityFact
    public let toolLifecycle: ProviderCapabilityFact
    public let currentTarget: ProviderCapabilityFact

    public init(
        plan: ProviderCapabilityFact = .unknown,
        subagents: ProviderCapabilityFact = .unknown,
        approvals: ProviderCapabilityFact = .unknown,
        transcriptSlice: ProviderCapabilityFact = .unknown,
        toolLifecycle: ProviderCapabilityFact = .unknown,
        currentTarget: ProviderCapabilityFact = .unknown
    ) {
        self.plan = plan
        self.subagents = subagents
        self.approvals = approvals
        self.transcriptSlice = transcriptSlice
        self.toolLifecycle = toolLifecycle
        self.currentTarget = currentTarget
    }

    private enum CodingKeys: String, CodingKey {
        case plan, subagents, approvals, transcriptSlice, toolLifecycle, currentTarget
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        plan = (try? container.decode(ProviderCapabilityFact.self, forKey: .plan)) ?? .unknown
        subagents = (try? container.decode(
            ProviderCapabilityFact.self,
            forKey: .subagents
        )) ?? .unknown
        approvals = (try? container.decode(
            ProviderCapabilityFact.self,
            forKey: .approvals
        )) ?? .unknown
        transcriptSlice = (try? container.decode(
            ProviderCapabilityFact.self,
            forKey: .transcriptSlice
        )) ?? .unknown
        toolLifecycle = (try? container.decode(
            ProviderCapabilityFact.self,
            forKey: .toolLifecycle
        )) ?? .unknown
        currentTarget = (try? container.decode(
            ProviderCapabilityFact.self,
            forKey: .currentTarget
        )) ?? .unknown
    }

    public func capability(for feature: ProviderCapabilityFeature) -> ProviderCapabilityFact {
        switch feature {
        case .plan: plan
        case .subagents: subagents
        case .approvals: approvals
        case .transcriptSlice: transcriptSlice
        case .toolLifecycle: toolLifecycle
        case .currentTarget: currentTarget
        }
    }
}

public struct ProviderCapabilityMatrix: Codable, Equatable, Sendable {
    public static let supportedSchemaVersion = 1

    public let schemaVersion: Int
    public let providers: [String: ProviderCapabilitySet]

    public init(schemaVersion: Int, providers: [String: ProviderCapabilitySet]) {
        self.schemaVersion = schemaVersion
        self.providers = schemaVersion == Self.supportedSchemaVersion && providers.count <= 32
            ? providers
            : [:]
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, providers
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let schemaVersion = (try? container.decode(Int.self, forKey: .schemaVersion)) ?? 0
        let providers = (try? container.decode(
            [String: ProviderCapabilitySet].self,
            forKey: .providers
        )) ?? [:]
        self.init(schemaVersion: schemaVersion, providers: providers)
    }

    public func capability(
        for provider: ProviderKind,
        feature: ProviderCapabilityFeature
    ) -> ProviderCapabilityFact {
        providers[provider.rawValue]?.capability(for: feature) ?? .unknown
    }
}

public struct SnapshotCapabilities: Codable, Equatable, Sendable {
    public let codexConnector: CodexConnectorCapability?
    public let providerMatrix: ProviderCapabilityMatrix?

    public init(
        codexConnector: CodexConnectorCapability? = nil,
        providerMatrix: ProviderCapabilityMatrix? = nil
    ) {
        self.codexConnector = codexConnector
        self.providerMatrix = providerMatrix
    }

    private enum CodingKeys: String, CodingKey {
        case codexConnector, providerMatrix
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        codexConnector = try? container.decode(CodexConnectorCapability.self, forKey: .codexConnector)
        providerMatrix = try? container.decode(
            ProviderCapabilityMatrix.self,
            forKey: .providerMatrix
        )
    }
}


public struct TokenUsageProviderTotal: Codable, Equatable, Sendable, Identifiable {
    public let provider: String
    public let today: UInt64
    public let month: UInt64
    public let total: UInt64

    public var id: String { provider }

    public init(
        provider: String,
        total: UInt64,
        today: UInt64 = 0,
        month: UInt64 = 0
    ) {
        self.provider = provider
        self.today = today
        self.month = month
        self.total = total
    }

    private enum CodingKeys: String, CodingKey {
        case provider, today, month, total
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        provider = try values.decode(String.self, forKey: .provider)
        today = try values.decodeIfPresent(UInt64.self, forKey: .today) ?? 0
        month = try values.decodeIfPresent(UInt64.self, forKey: .month) ?? 0
        total = try values.decodeIfPresent(UInt64.self, forKey: .total) ?? 0
    }
}

public struct TokenUsageModelTotal: Codable, Equatable, Sendable, Identifiable {
    public let provider: String
    public let model: String
    public let total: UInt64
    public let inputTokens: UInt64?
    public let outputTokens: UInt64?
    public let cacheReadTokens: UInt64?
    public let cacheCreationTokens: UInt64?
    public let reasoningTokens: UInt64?
    public let estimatedCostUsdMicros: UInt64?
    public let pricedTokens: UInt64?
    public let unpricedTokens: UInt64?

    public var id: String { "\(provider):\(model)" }

    public init(
        provider: String,
        model: String,
        total: UInt64,
        inputTokens: UInt64? = nil,
        outputTokens: UInt64? = nil,
        cacheReadTokens: UInt64? = nil,
        cacheCreationTokens: UInt64? = nil,
        reasoningTokens: UInt64? = nil,
        estimatedCostUsdMicros: UInt64? = nil,
        pricedTokens: UInt64? = nil,
        unpricedTokens: UInt64? = nil
    ) {
        self.provider = provider
        self.model = model
        self.total = total
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheReadTokens = cacheReadTokens
        self.cacheCreationTokens = cacheCreationTokens
        self.reasoningTokens = reasoningTokens
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricedTokens = pricedTokens
        self.unpricedTokens = unpricedTokens
    }
}

public struct TokenUsageDayProviderTotal: Codable, Equatable, Sendable, Identifiable {
    public let provider: String
    public let total: UInt64
    public let estimatedCostUsdMicros: UInt64?
    public let pricedTokens: UInt64
    public let unpricedTokens: UInt64
    public let messageCount: UInt64

    public var id: String { provider }

    public init(
        provider: String,
        total: UInt64,
        estimatedCostUsdMicros: UInt64? = nil,
        pricedTokens: UInt64? = nil,
        unpricedTokens: UInt64? = nil,
        messageCount: UInt64 = 0
    ) {
        self.provider = provider
        self.total = total
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricedTokens = pricedTokens ?? (estimatedCostUsdMicros == nil ? 0 : total)
        self.unpricedTokens = unpricedTokens ?? (estimatedCostUsdMicros == nil ? total : 0)
        self.messageCount = messageCount
    }

    private enum CodingKeys: String, CodingKey {
        case provider, total, estimatedCostUsdMicros, pricedTokens, unpricedTokens, messageCount
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        provider = try values.decode(String.self, forKey: .provider)
        total = try values.decodeIfPresent(UInt64.self, forKey: .total) ?? 0
        estimatedCostUsdMicros = try values.decodeIfPresent(
            UInt64.self,
            forKey: .estimatedCostUsdMicros
        )
        pricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .pricedTokens)
            ?? (estimatedCostUsdMicros == nil ? 0 : total)
        unpricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .unpricedTokens)
            ?? (estimatedCostUsdMicros == nil ? total : 0)
        messageCount = try values.decodeIfPresent(UInt64.self, forKey: .messageCount) ?? 0
    }
}

public struct TokenUsageDayModelTotal: Codable, Equatable, Sendable, Identifiable {
    public let provider: String
    public let model: String
    public let total: UInt64
    public let inputTokens: UInt64?
    public let outputTokens: UInt64?
    public let cacheReadTokens: UInt64?
    public let cacheCreationTokens: UInt64?
    public let reasoningTokens: UInt64?
    public let estimatedCostUsdMicros: UInt64?
    public let pricedTokens: UInt64
    public let unpricedTokens: UInt64
    public let messageCount: UInt64

    public var id: String { "\(provider):\(model)" }

    public init(
        provider: String,
        model: String,
        total: UInt64,
        inputTokens: UInt64? = nil,
        outputTokens: UInt64? = nil,
        cacheReadTokens: UInt64? = nil,
        cacheCreationTokens: UInt64? = nil,
        reasoningTokens: UInt64? = nil,
        estimatedCostUsdMicros: UInt64? = nil,
        pricedTokens: UInt64? = nil,
        unpricedTokens: UInt64? = nil,
        messageCount: UInt64 = 0
    ) {
        self.provider = provider
        self.model = model
        self.total = total
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheReadTokens = cacheReadTokens
        self.cacheCreationTokens = cacheCreationTokens
        self.reasoningTokens = reasoningTokens
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricedTokens = pricedTokens ?? (estimatedCostUsdMicros == nil ? 0 : total)
        self.unpricedTokens = unpricedTokens ?? (estimatedCostUsdMicros == nil ? total : 0)
        self.messageCount = messageCount
    }

    private enum CodingKeys: String, CodingKey {
        case provider, model, total, inputTokens, outputTokens, cacheReadTokens
        case cacheCreationTokens, reasoningTokens, estimatedCostUsdMicros
        case pricedTokens, unpricedTokens, messageCount
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        provider = try values.decode(String.self, forKey: .provider)
        model = try values.decode(String.self, forKey: .model)
        total = try values.decodeIfPresent(UInt64.self, forKey: .total) ?? 0
        inputTokens = try values.decodeIfPresent(UInt64.self, forKey: .inputTokens)
        outputTokens = try values.decodeIfPresent(UInt64.self, forKey: .outputTokens)
        cacheReadTokens = try values.decodeIfPresent(UInt64.self, forKey: .cacheReadTokens)
        cacheCreationTokens = try values.decodeIfPresent(
            UInt64.self,
            forKey: .cacheCreationTokens
        )
        reasoningTokens = try values.decodeIfPresent(UInt64.self, forKey: .reasoningTokens)
        estimatedCostUsdMicros = try values.decodeIfPresent(
            UInt64.self,
            forKey: .estimatedCostUsdMicros
        )
        pricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .pricedTokens)
            ?? (estimatedCostUsdMicros == nil ? 0 : total)
        unpricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .unpricedTokens)
            ?? (estimatedCostUsdMicros == nil ? total : 0)
        messageCount = try values.decodeIfPresent(UInt64.self, forKey: .messageCount) ?? 0
    }
}

public struct TokenUsageDayTotal: Codable, Equatable, Sendable, Identifiable {
    public let day: String
    public let total: UInt64
    public let estimatedCostUsdMicros: UInt64?
    public let pricedTokens: UInt64
    public let unpricedTokens: UInt64
    public let messageCount: UInt64
    public let byProvider: [TokenUsageDayProviderTotal]
    public let byModel: [TokenUsageDayModelTotal]

    public var id: String { day }

    public init(
        day: String,
        total: UInt64,
        estimatedCostUsdMicros: UInt64? = nil,
        pricedTokens: UInt64? = nil,
        unpricedTokens: UInt64? = nil,
        messageCount: UInt64 = 0,
        byProvider: [TokenUsageDayProviderTotal] = [],
        byModel: [TokenUsageDayModelTotal] = []
    ) {
        self.day = day
        self.total = total
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricedTokens = pricedTokens ?? (estimatedCostUsdMicros == nil ? 0 : total)
        self.unpricedTokens = unpricedTokens ?? (estimatedCostUsdMicros == nil ? total : 0)
        self.messageCount = messageCount
        self.byProvider = byProvider
        self.byModel = byModel
    }

    private enum CodingKeys: String, CodingKey {
        case day, total, estimatedCostUsdMicros, pricedTokens, unpricedTokens
        case messageCount, byProvider, byModel
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        day = try values.decode(String.self, forKey: .day)
        total = try values.decodeIfPresent(UInt64.self, forKey: .total) ?? 0
        estimatedCostUsdMicros = try values.decodeIfPresent(
            UInt64.self,
            forKey: .estimatedCostUsdMicros
        )
        pricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .pricedTokens)
            ?? (estimatedCostUsdMicros == nil ? 0 : total)
        unpricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .unpricedTokens)
            ?? (estimatedCostUsdMicros == nil ? total : 0)
        messageCount = try values.decodeIfPresent(UInt64.self, forKey: .messageCount) ?? 0
        byProvider = try values.decodeIfPresent(
            [TokenUsageDayProviderTotal].self,
            forKey: .byProvider
        ) ?? []
        byModel = try values.decodeIfPresent(
            [TokenUsageDayModelTotal].self,
            forKey: .byModel
        ) ?? []
    }
}

public struct TokenUsagePricingSource: Codable, Equatable, Sendable, Identifiable {
    public let costKind: String
    public let source: String
    public let tokenTotal: UInt64
    public let estimatedCostUsdMicros: UInt64

    public var id: String { "\(costKind):\(source)" }

    public init(
        costKind: String,
        source: String,
        tokenTotal: UInt64,
        estimatedCostUsdMicros: UInt64
    ) {
        self.costKind = costKind
        self.source = source
        self.tokenTotal = tokenTotal
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
    }
}

public struct TokenUsageAnomaly: Codable, Equatable, Sendable, Identifiable {
    public let code: String
    public let severity: String
    public let scope: String
    public let day: String?
    public let observed: UInt64?
    public let expected: UInt64?

    public var id: String {
        "\(severity):\(scope):\(code):\(day ?? "all")"
    }

    public init(
        code: String,
        severity: String,
        scope: String,
        day: String? = nil,
        observed: UInt64? = nil,
        expected: UInt64? = nil
    ) {
        self.code = code
        self.severity = severity
        self.scope = scope
        self.day = day
        self.observed = observed
        self.expected = expected
    }
}

public struct TokenUsageTotals: Codable, Equatable, Sendable {
    public let today: UInt64
    public let month: UInt64
    public let total: UInt64
    public let activeDays: UInt64
    public let currentStreak: UInt64
    public let todayActiveTimeSeconds: UInt64
    public let monthActiveTimeSeconds: UInt64
    public let activeTimeSeconds: UInt64
    public let todayExecutionTimeSeconds: UInt64
    public let monthExecutionTimeSeconds: UInt64
    public let executionTimeSeconds: UInt64
    public let turnCount: UInt64
    public let messageCount: UInt64
    public let pricedTokens: UInt64
    public let unpricedTokens: UInt64
    public let estimatedCostUsdMicros: UInt64?
    public let pricingSources: [TokenUsagePricingSource]
    public let anomalies: [TokenUsageAnomaly]
    public let anomalyCount: UInt64
    public let suspectCount: UInt64
    public let peakDay: String?
    public let peakDayTotal: UInt64
    public let recordedFrom: UInt64?
    public let capturedAt: UInt64?
    public let byProvider: [TokenUsageProviderTotal]
    public let byModel: [TokenUsageModelTotal]
    public let recentDays: [TokenUsageDayTotal]
    public let detailRecordedFrom: UInt64?
    public let collectionState: String
    public let dataQuality: String
    public let collectionInProgress: Bool
    public let consecutiveFailures: UInt64
    public let lastSuccessfulAt: UInt64?
    public let lastAuditedAt: UInt64?

    public init(
        today: UInt64,
        month: UInt64,
        total: UInt64,
        activeDays: UInt64 = 0,
        currentStreak: UInt64 = 0,
        todayActiveTimeSeconds: UInt64 = 0,
        monthActiveTimeSeconds: UInt64 = 0,
        activeTimeSeconds: UInt64 = 0,
        todayExecutionTimeSeconds: UInt64 = 0,
        monthExecutionTimeSeconds: UInt64 = 0,
        executionTimeSeconds: UInt64 = 0,
        turnCount: UInt64 = 0,
        messageCount: UInt64 = 0,
        pricedTokens: UInt64 = 0,
        unpricedTokens: UInt64 = 0,
        estimatedCostUsdMicros: UInt64? = nil,
        pricingSources: [TokenUsagePricingSource] = [],
        anomalies: [TokenUsageAnomaly] = [],
        anomalyCount: UInt64? = nil,
        suspectCount: UInt64? = nil,
        peakDay: String? = nil,
        peakDayTotal: UInt64 = 0,
        recordedFrom: UInt64? = nil,
        capturedAt: UInt64? = nil,
        byProvider: [TokenUsageProviderTotal] = [],
        byModel: [TokenUsageModelTotal] = [],
        recentDays: [TokenUsageDayTotal] = [],
        detailRecordedFrom: UInt64? = nil,
        collectionState: String = "pending",
        dataQuality: String = "pending",
        collectionInProgress: Bool = false,
        consecutiveFailures: UInt64 = 0,
        lastSuccessfulAt: UInt64? = nil,
        lastAuditedAt: UInt64? = nil
    ) {
        self.today = today
        self.month = month
        self.total = total
        self.activeDays = activeDays
        self.currentStreak = currentStreak
        self.todayActiveTimeSeconds = todayActiveTimeSeconds
        self.monthActiveTimeSeconds = monthActiveTimeSeconds
        self.activeTimeSeconds = activeTimeSeconds
        self.todayExecutionTimeSeconds = todayExecutionTimeSeconds
        self.monthExecutionTimeSeconds = monthExecutionTimeSeconds
        self.executionTimeSeconds = executionTimeSeconds
        self.turnCount = turnCount
        self.messageCount = messageCount
        self.pricedTokens = pricedTokens
        self.unpricedTokens = unpricedTokens
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricingSources = pricingSources
        self.anomalies = anomalies
        self.anomalyCount = anomalyCount ?? UInt64(anomalies.count)
        self.suspectCount = suspectCount ?? UInt64(
            anomalies.filter { $0.severity == "suspect" }.count
        )
        self.peakDay = peakDay
        self.peakDayTotal = peakDayTotal
        self.recordedFrom = recordedFrom
        self.capturedAt = capturedAt
        self.byProvider = byProvider
        self.byModel = byModel
        self.recentDays = recentDays
        self.detailRecordedFrom = detailRecordedFrom
        self.collectionState = collectionState
        self.dataQuality = dataQuality
        self.collectionInProgress = collectionInProgress
        self.consecutiveFailures = consecutiveFailures
        self.lastSuccessfulAt = lastSuccessfulAt
        self.lastAuditedAt = lastAuditedAt
    }

    private enum CodingKeys: String, CodingKey {
        case today, month, total, activeDays, currentStreak
        case todayActiveTimeSeconds, monthActiveTimeSeconds, activeTimeSeconds
        case todayExecutionTimeSeconds, monthExecutionTimeSeconds, executionTimeSeconds
        case turnCount, messageCount, pricedTokens, unpricedTokens, estimatedCostUsdMicros
        case pricingSources, anomalies, anomalyCount, suspectCount
        case peakDay, peakDayTotal, recordedFrom, capturedAt
        case byProvider, byModel, recentDays, detailRecordedFrom
        case collectionState, dataQuality, collectionInProgress
        case consecutiveFailures, lastSuccessfulAt, lastAuditedAt
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        today = try values.decodeIfPresent(UInt64.self, forKey: .today) ?? 0
        month = try values.decodeIfPresent(UInt64.self, forKey: .month) ?? 0
        total = try values.decodeIfPresent(UInt64.self, forKey: .total) ?? 0
        activeDays = try values.decodeIfPresent(UInt64.self, forKey: .activeDays) ?? 0
        currentStreak = try values.decodeIfPresent(
            UInt64.self,
            forKey: .currentStreak
        ) ?? 0
        todayActiveTimeSeconds = try values.decodeIfPresent(
            UInt64.self,
            forKey: .todayActiveTimeSeconds
        ) ?? 0
        monthActiveTimeSeconds = try values.decodeIfPresent(
            UInt64.self,
            forKey: .monthActiveTimeSeconds
        ) ?? 0
        activeTimeSeconds = try values.decodeIfPresent(
            UInt64.self,
            forKey: .activeTimeSeconds
        ) ?? 0
        todayExecutionTimeSeconds = try values.decodeIfPresent(
            UInt64.self,
            forKey: .todayExecutionTimeSeconds
        ) ?? 0
        monthExecutionTimeSeconds = try values.decodeIfPresent(
            UInt64.self,
            forKey: .monthExecutionTimeSeconds
        ) ?? 0
        executionTimeSeconds = try values.decodeIfPresent(
            UInt64.self,
            forKey: .executionTimeSeconds
        ) ?? 0
        turnCount = try values.decodeIfPresent(UInt64.self, forKey: .turnCount) ?? 0
        messageCount = try values.decodeIfPresent(UInt64.self, forKey: .messageCount) ?? 0
        pricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .pricedTokens) ?? 0
        unpricedTokens = try values.decodeIfPresent(UInt64.self, forKey: .unpricedTokens) ?? 0
        estimatedCostUsdMicros = try values.decodeIfPresent(
            UInt64.self,
            forKey: .estimatedCostUsdMicros
        )
        pricingSources = try values.decodeIfPresent(
            [TokenUsagePricingSource].self,
            forKey: .pricingSources
        ) ?? []
        anomalies = try values.decodeIfPresent(
            [TokenUsageAnomaly].self,
            forKey: .anomalies
        ) ?? []
        anomalyCount = try values.decodeIfPresent(UInt64.self, forKey: .anomalyCount)
            ?? UInt64(anomalies.count)
        suspectCount = try values.decodeIfPresent(UInt64.self, forKey: .suspectCount)
            ?? UInt64(anomalies.filter { $0.severity == "suspect" }.count)
        peakDay = try values.decodeIfPresent(String.self, forKey: .peakDay)
        peakDayTotal = try values.decodeIfPresent(
            UInt64.self,
            forKey: .peakDayTotal
        ) ?? 0
        recordedFrom = try values.decodeIfPresent(UInt64.self, forKey: .recordedFrom)
        capturedAt = try values.decodeIfPresent(UInt64.self, forKey: .capturedAt)
        byProvider = try values.decodeIfPresent(
            [TokenUsageProviderTotal].self,
            forKey: .byProvider
        ) ?? []
        byModel = try values.decodeIfPresent(
            [TokenUsageModelTotal].self,
            forKey: .byModel
        ) ?? []
        recentDays = try values.decodeIfPresent(
            [TokenUsageDayTotal].self,
            forKey: .recentDays
        ) ?? []
        detailRecordedFrom = try values.decodeIfPresent(
            UInt64.self,
            forKey: .detailRecordedFrom
        )
        collectionState = try values.decodeIfPresent(
            String.self,
            forKey: .collectionState
        ) ?? "pending"
        dataQuality = try values.decodeIfPresent(String.self, forKey: .dataQuality)
            ?? (collectionState == "ready" ? "verified" : collectionState)
        collectionInProgress = try values.decodeIfPresent(
            Bool.self,
            forKey: .collectionInProgress
        ) ?? false
        consecutiveFailures = try values.decodeIfPresent(
            UInt64.self,
            forKey: .consecutiveFailures
        ) ?? 0
        lastSuccessfulAt = try values.decodeIfPresent(
            UInt64.self,
            forKey: .lastSuccessfulAt
        )
        lastAuditedAt = try values.decodeIfPresent(UInt64.self, forKey: .lastAuditedAt)
            ?? lastSuccessfulAt
    }

    public static let empty = TokenUsageTotals(today: 0, month: 0, total: 0)
}

public struct TokenUsageProjectTotal: Codable, Equatable, Sendable, Identifiable {
    public var id: String { project }
    public let project: String
    public let total: UInt64
    public let taskCount: UInt64
    public let sessionCount: UInt64
    public let capturedAt: UInt64?

    private enum CodingKeys: String, CodingKey {
        case project, total, taskCount, sessionCount, capturedAt
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        project = try values.decode(String.self, forKey: .project)
        total = try values.decode(UInt64.self, forKey: .total)
        taskCount = try values.decode(UInt64.self, forKey: .taskCount)
        sessionCount = try values.decodeIfPresent(UInt64.self, forKey: .sessionCount)
            ?? taskCount
        capturedAt = try values.decodeIfPresent(UInt64.self, forKey: .capturedAt)
    }
}

public struct TokenUsageTaskTotal: Codable, Equatable, Sendable, Identifiable {
    public var id: String { sessionId }
    public let sessionId: String
    public let provider: String
    public let project: String?
    public let title: String?
    public let model: String?
    public let total: UInt64
    public let capturedAt: UInt64?
}

public struct TokenUsageBurnRate: Codable, Equatable, Sendable, Identifiable {
    public var id: String { "\(sessionId):\(turnId)" }
    public let sessionId: String
    public let turnId: String
    public let provider: String
    public let project: String?
    public let title: String?
    public let windowSeconds: UInt64
    public let tokenDelta: UInt64
    public let tokensPerMinute: UInt64
    public let sampleCount: UInt64
    public let baselineTokensPerMinute: UInt64?
    public let ratioBasisPoints: UInt64?
    public let state: String
    public let capturedAt: UInt64
    public let thresholdExceeded: Bool
}

public struct TokenUsageDecisionSummary: Codable, Equatable, Sendable {
    public let schemaVersion: UInt16
    public let source: String
    public let generatedAt: UInt64
    public let freshness: String
    public let capturedAt: UInt64?
    public let totalTokens: UInt64
    public let attributedTokens: UInt64
    public let unattributedTokens: UInt64
    public let attributionCoverageBasisPoints: UInt64
    public let projectAttributedTokens: UInt64
    public let projectUnattributedTokens: UInt64
    public let projectAttributionCoverageBasisPoints: UInt64
    public let taskAttributedTokens: UInt64
    public let taskUnattributedTokens: UInt64
    public let taskAttributionCoverageBasisPoints: UInt64
    public let projectTotals: [TokenUsageProjectTotal]
    public let taskTotals: [TokenUsageTaskTotal]
    public let burnRates: [TokenUsageBurnRate]
    public let thresholdTokensPerMinute: UInt64?

    public init(
        schemaVersion: UInt16,
        source: String,
        generatedAt: UInt64,
        freshness: String,
        capturedAt: UInt64?,
        totalTokens: UInt64,
        attributedTokens: UInt64,
        unattributedTokens: UInt64,
        attributionCoverageBasisPoints: UInt64,
        projectAttributedTokens: UInt64,
        projectUnattributedTokens: UInt64,
        projectAttributionCoverageBasisPoints: UInt64,
        taskAttributedTokens: UInt64,
        taskUnattributedTokens: UInt64,
        taskAttributionCoverageBasisPoints: UInt64,
        projectTotals: [TokenUsageProjectTotal],
        taskTotals: [TokenUsageTaskTotal],
        burnRates: [TokenUsageBurnRate],
        thresholdTokensPerMinute: UInt64?
    ) {
        self.schemaVersion = schemaVersion
        self.source = source
        self.generatedAt = generatedAt
        self.freshness = freshness
        self.capturedAt = capturedAt
        self.totalTokens = totalTokens
        self.attributedTokens = attributedTokens
        self.unattributedTokens = unattributedTokens
        self.attributionCoverageBasisPoints = attributionCoverageBasisPoints
        self.projectAttributedTokens = projectAttributedTokens
        self.projectUnattributedTokens = projectUnattributedTokens
        self.projectAttributionCoverageBasisPoints = projectAttributionCoverageBasisPoints
        self.taskAttributedTokens = taskAttributedTokens
        self.taskUnattributedTokens = taskUnattributedTokens
        self.taskAttributionCoverageBasisPoints = taskAttributionCoverageBasisPoints
        self.projectTotals = projectTotals
        self.taskTotals = taskTotals
        self.burnRates = burnRates
        self.thresholdTokensPerMinute = thresholdTokensPerMinute
    }

    private enum CodingKeys: String, CodingKey {
        case schemaVersion, source, generatedAt, freshness, capturedAt
        case totalTokens, attributedTokens, unattributedTokens
        case attributionCoverageBasisPoints
        case projectAttributedTokens, projectUnattributedTokens
        case projectAttributionCoverageBasisPoints
        case taskAttributedTokens, taskUnattributedTokens
        case taskAttributionCoverageBasisPoints
        case projectTotals, taskTotals, burnRates, thresholdTokensPerMinute
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        schemaVersion = try values.decode(UInt16.self, forKey: .schemaVersion)
        source = try values.decode(String.self, forKey: .source)
        generatedAt = try values.decode(UInt64.self, forKey: .generatedAt)
        freshness = try values.decode(String.self, forKey: .freshness)
        capturedAt = try values.decodeIfPresent(UInt64.self, forKey: .capturedAt)
        totalTokens = try values.decode(UInt64.self, forKey: .totalTokens)
        attributedTokens = try values.decode(UInt64.self, forKey: .attributedTokens)
        unattributedTokens = try values.decode(UInt64.self, forKey: .unattributedTokens)
        attributionCoverageBasisPoints = try values.decode(
            UInt64.self,
            forKey: .attributionCoverageBasisPoints
        )
        projectAttributedTokens = try values.decodeIfPresent(
            UInt64.self,
            forKey: .projectAttributedTokens
        ) ?? attributedTokens
        projectUnattributedTokens = try values.decodeIfPresent(
            UInt64.self,
            forKey: .projectUnattributedTokens
        ) ?? unattributedTokens
        projectAttributionCoverageBasisPoints = try values.decodeIfPresent(
            UInt64.self,
            forKey: .projectAttributionCoverageBasisPoints
        ) ?? attributionCoverageBasisPoints
        taskAttributedTokens = try values.decodeIfPresent(
            UInt64.self,
            forKey: .taskAttributedTokens
        ) ?? attributedTokens
        taskUnattributedTokens = try values.decodeIfPresent(
            UInt64.self,
            forKey: .taskUnattributedTokens
        ) ?? unattributedTokens
        taskAttributionCoverageBasisPoints = try values.decodeIfPresent(
            UInt64.self,
            forKey: .taskAttributionCoverageBasisPoints
        ) ?? attributionCoverageBasisPoints
        projectTotals = try values.decode([TokenUsageProjectTotal].self, forKey: .projectTotals)
        taskTotals = try values.decode([TokenUsageTaskTotal].self, forKey: .taskTotals)
        burnRates = try values.decode([TokenUsageBurnRate].self, forKey: .burnRates)
        thresholdTokensPerMinute = try values.decodeIfPresent(
            UInt64.self,
            forKey: .thresholdTokensPerMinute
        )
    }

    public static let empty = TokenUsageDecisionSummary(
        schemaVersion: 1,
        source: "runtime:canonical_session_ledger",
        generatedAt: 0,
        freshness: "unavailable",
        capturedAt: nil,
        totalTokens: 0,
        attributedTokens: 0,
        unattributedTokens: 0,
        attributionCoverageBasisPoints: 0,
        projectAttributedTokens: 0,
        projectUnattributedTokens: 0,
        projectAttributionCoverageBasisPoints: 0,
        taskAttributedTokens: 0,
        taskUnattributedTokens: 0,
        taskAttributionCoverageBasisPoints: 0,
        projectTotals: [],
        taskTotals: [],
        burnRates: [],
        thresholdTokensPerMinute: nil
    )
}

public struct Snapshot: Codable, Equatable, Sendable {
    public let sessions: [SessionRecord]
    public let attention: [AttentionRecord]
    public let commands: [CommandRecord]
    public let quota: [QuotaEntry]
    public let tokenUsage: TokenUsageTotals
    public let tokenDecision: TokenUsageDecisionSummary
    public let stats: SnapshotStats
    public let capabilities: SnapshotCapabilities?

    public init(
        sessions: [SessionRecord],
        attention: [AttentionRecord],
        commands: [CommandRecord],
        quota: [QuotaEntry],
        tokenUsage: TokenUsageTotals = .empty,
        tokenDecision: TokenUsageDecisionSummary = .empty,
        stats: SnapshotStats,
        capabilities: SnapshotCapabilities? = nil
    ) {
        self.sessions = sessions
        self.attention = attention
        self.commands = commands
        self.quota = quota
        self.tokenUsage = tokenUsage
        self.tokenDecision = tokenDecision
        self.stats = stats
        self.capabilities = capabilities
    }

    private enum CodingKeys: String, CodingKey {
        case sessions, attention, commands, quota, tokenUsage, tokenDecision
        case stats, capabilities
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        sessions = try container.decode([SessionRecord].self, forKey: .sessions)
        attention = try container.decode([AttentionRecord].self, forKey: .attention)
        commands = try container.decode([CommandRecord].self, forKey: .commands)
        quota = try container.decode([QuotaEntry].self, forKey: .quota)
        tokenUsage = try container.decodeIfPresent(
            TokenUsageTotals.self,
            forKey: .tokenUsage
        ) ?? .empty
        tokenDecision = try container.decodeIfPresent(
            TokenUsageDecisionSummary.self,
            forKey: .tokenDecision
        ) ?? .empty
        stats = try container.decode(SnapshotStats.self, forKey: .stats)
        capabilities = try container.decodeIfPresent(
            SnapshotCapabilities.self,
            forKey: .capabilities
        )
    }

    public func providerCapability(
        for provider: ProviderKind,
        feature: ProviderCapabilityFeature
    ) -> ProviderCapabilityFact {
        capabilities?.providerMatrix?.capability(for: provider, feature: feature) ?? .unknown
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

public struct CompanionPairingResponse: Codable, Equatable, Sendable {
    public let enrollmentCode: String
    public let expiresAt: UInt64
    public let scopes: [String]
    public let endpoint: String
}

public struct CompanionConnection: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let clientName: String
    public let scopes: [String]
    public let createdAt: UInt64
}

public struct CompanionConnectionsResponse: Codable, Equatable, Sendable {
    public let connections: [CompanionConnection]
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

public enum TaskCardDisplayPresets {
    public static let concise = [
        "project", "task", "model", "activity", "plan", "sessionTokens", "context",
        "taskFlow", "workflow",
    ]
    public static let detailed = [
        "project", "task", "model", "activity", "plan", "sessionTokens", "turnTokens",
        "inputOutputTokens", "cacheTokens", "reasoningTokens", "cost", "context", "tool",
        "currentTarget", "subagents", "environment", "recovery", "control", "jump",
        "taskFlow", "workflow",
    ]
    public static let developer = detailed + [
        "permissionMode", "titleSource", "sessionId", "providerSessionId", "providerTurnId",
        "lastEventAt",
    ]
    public static let all = [
        "concise": concise,
        "detailed": detailed,
        "developer": developer,
    ]
}

public enum QuotaDisplayMode: String, Codable, CaseIterable, Sendable {
    case full = "standard"
    case compact = "twoLine"
    // Preserve the previous one-line mode's persisted raw value.
    case singleLine = "compact"
}

public enum TokenUsageDisplayMode: String, Codable, CaseIterable, Sendable {
    case full = "standard"
    case compact
    case hidden
}

public enum TokenUsageUnitStyle: String, Codable, CaseIterable, Sendable {
    case automatic
    case western
    case eastAsian
}

public enum CompletionTaskHideMode: String, Codable, CaseIterable, Sendable {
    case afterConfirmation
    case afterDelay
}

public struct UISettings: Codable, Equatable, Sendable {
    public var notificationRules: NotificationRules
    public var soundEnabled: Bool
    public var providerMuted: ProviderMuted
    public var codexEnhancedActivity: Bool
    public var retentionDays: UInt32
    public var displayProfile: String
    public var taskCardFields: [String]
    public var displayFieldsVersion: UInt32?
    public var quotaDisplayMode: QuotaDisplayMode
    public var tokenUsageDisplayMode: TokenUsageDisplayMode
    public var tokenUsageComponentsVisible: Bool
    public var tokenUsageHeatmapVisible: Bool
    public var tokenUsageCostVisible: Bool
    public var tokenUsageObservedTimeVisible: Bool
    public var tokenUsageExecutionTimeVisible: Bool
    public var tokenUsageUnitStyle: TokenUsageUnitStyle
    public var tokenUsageTaskProjectVisible: Bool
    public var tokenUsageBurnRateVisible: Bool
    public var tokenUsageAnomalyVisible: Bool
    public var tokenThresholdNotificationsEnabled: Bool
    public var tokenThresholdTokensPerMinute: UInt64
    public var completionTaskHideMode: CompletionTaskHideMode
    public var completionAutoHideMinutes: UInt32

    public init(
        notificationRules: NotificationRules = NotificationRules(),
        soundEnabled: Bool = true,
        providerMuted: ProviderMuted = ProviderMuted(),
        codexEnhancedActivity: Bool = true,
        retentionDays: UInt32 = 90,
        displayProfile: String = "detailed",
        taskCardFields: [String] = TaskCardDisplayPresets.detailed,
        displayFieldsVersion: UInt32? = 5,
        quotaDisplayMode: QuotaDisplayMode = .full,
        tokenUsageDisplayMode: TokenUsageDisplayMode = .full,
        tokenUsageComponentsVisible: Bool = true,
        tokenUsageHeatmapVisible: Bool = true,
        tokenUsageCostVisible: Bool = true,
        tokenUsageObservedTimeVisible: Bool = true,
        tokenUsageExecutionTimeVisible: Bool = true,
        tokenUsageUnitStyle: TokenUsageUnitStyle = .automatic,
        tokenUsageTaskProjectVisible: Bool = true,
        tokenUsageBurnRateVisible: Bool = true,
        tokenUsageAnomalyVisible: Bool = true,
        tokenThresholdNotificationsEnabled: Bool = false,
        tokenThresholdTokensPerMinute: UInt64 = 250_000,
        completionTaskHideMode: CompletionTaskHideMode = .afterConfirmation,
        completionAutoHideMinutes: UInt32 = 30
    ) {
        self.notificationRules = notificationRules
        self.soundEnabled = soundEnabled
        self.providerMuted = providerMuted
        self.codexEnhancedActivity = codexEnhancedActivity
        self.retentionDays = retentionDays
        self.displayProfile = displayProfile
        self.taskCardFields = taskCardFields
        self.displayFieldsVersion = displayFieldsVersion
        self.quotaDisplayMode = quotaDisplayMode
        self.tokenUsageDisplayMode = tokenUsageDisplayMode
        self.tokenUsageComponentsVisible = tokenUsageComponentsVisible
        self.tokenUsageHeatmapVisible = tokenUsageHeatmapVisible
        self.tokenUsageCostVisible = tokenUsageCostVisible
        self.tokenUsageObservedTimeVisible = tokenUsageObservedTimeVisible
        self.tokenUsageExecutionTimeVisible = tokenUsageExecutionTimeVisible
        self.tokenUsageUnitStyle = tokenUsageUnitStyle
        self.tokenUsageTaskProjectVisible = tokenUsageTaskProjectVisible
        self.tokenUsageBurnRateVisible = tokenUsageBurnRateVisible
        self.tokenUsageAnomalyVisible = tokenUsageAnomalyVisible
        self.tokenThresholdNotificationsEnabled = tokenThresholdNotificationsEnabled
        self.tokenThresholdTokensPerMinute = tokenThresholdTokensPerMinute
        self.completionTaskHideMode = completionTaskHideMode
        self.completionAutoHideMinutes = completionAutoHideMinutes
    }

    public static let defaults = UISettings()

    private enum CodingKeys: String, CodingKey {
        case notificationRules
        case soundEnabled
        case providerMuted
        case codexEnhancedActivity
        case retentionDays
        case displayProfile
        case taskCardFields
        case displayFieldsVersion
        case quotaDisplayMode
        case tokenUsageDisplayMode
        case tokenUsageComponentsVisible
        case tokenUsageHeatmapVisible
        case tokenUsageCostVisible
        case tokenUsageObservedTimeVisible
        case tokenUsageExecutionTimeVisible
        case tokenUsageUnitStyle
        case tokenUsageTaskProjectVisible
        case tokenUsageBurnRateVisible
        case tokenUsageAnomalyVisible
        case tokenThresholdNotificationsEnabled
        case tokenThresholdTokensPerMinute
        case completionTaskHideMode
        case completionAutoHideMinutes
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        notificationRules = try container.decode(NotificationRules.self, forKey: .notificationRules)
        soundEnabled = try container.decode(Bool.self, forKey: .soundEnabled)
        providerMuted = try container.decode(ProviderMuted.self, forKey: .providerMuted)
        codexEnhancedActivity = try container.decode(Bool.self, forKey: .codexEnhancedActivity)
        retentionDays = try container.decode(UInt32.self, forKey: .retentionDays)
        displayProfile = try container.decode(String.self, forKey: .displayProfile)
        taskCardFields = try container.decode([String].self, forKey: .taskCardFields)
        displayFieldsVersion = try container.decodeIfPresent(UInt32.self, forKey: .displayFieldsVersion)
        quotaDisplayMode = try container.decodeIfPresent(
            QuotaDisplayMode.self,
            forKey: .quotaDisplayMode
        ) ?? .full
        tokenUsageDisplayMode = try container.decodeIfPresent(
            TokenUsageDisplayMode.self,
            forKey: .tokenUsageDisplayMode
        ) ?? .full
        tokenUsageComponentsVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageComponentsVisible
        ) ?? true
        tokenUsageHeatmapVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageHeatmapVisible
        ) ?? true
        tokenUsageCostVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageCostVisible
        ) ?? true
        tokenUsageObservedTimeVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageObservedTimeVisible
        ) ?? true
        tokenUsageExecutionTimeVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageExecutionTimeVisible
        ) ?? true
        tokenUsageUnitStyle = try container.decodeIfPresent(
            TokenUsageUnitStyle.self,
            forKey: .tokenUsageUnitStyle
        ) ?? .automatic
        tokenUsageTaskProjectVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageTaskProjectVisible
        ) ?? true
        tokenUsageBurnRateVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageBurnRateVisible
        ) ?? true
        tokenUsageAnomalyVisible = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenUsageAnomalyVisible
        ) ?? true
        tokenThresholdNotificationsEnabled = try container.decodeIfPresent(
            Bool.self,
            forKey: .tokenThresholdNotificationsEnabled
        ) ?? false
        tokenThresholdTokensPerMinute = try container.decodeIfPresent(
            UInt64.self,
            forKey: .tokenThresholdTokensPerMinute
        ) ?? 250_000
        completionTaskHideMode = try container.decodeIfPresent(
            CompletionTaskHideMode.self,
            forKey: .completionTaskHideMode
        ) ?? .afterConfirmation
        completionAutoHideMinutes = try container.decodeIfPresent(
            UInt32.self,
            forKey: .completionAutoHideMinutes
        ) ?? 30
    }
}

public struct DisplayField: Codable, Equatable, Sendable, Identifiable {
    public let id: String
    public let label: String
    public let level: String
    public let placement: String?
    public let description: String?

    public init(
        id: String,
        label: String,
        level: String,
        placement: String? = nil,
        description: String? = nil
    ) {
        self.id = id
        self.label = label
        self.level = level
        self.placement = placement
        self.description = description
    }
}

public struct ClaudeQuotaBridge: Codable, Equatable, Sendable {
    public let status: String
    public let configPath: String?
    public let helperPath: String?
    public let customConflict: Bool?
}

public struct BackupSummary: Codable, Equatable, Sendable {
    public let count: UInt64
    public let totalBytes: UInt64

    public init(count: UInt64, totalBytes: UInt64) {
        self.count = count
        self.totalBytes = totalBytes
    }

    public static let empty = BackupSummary(count: 0, totalBytes: 0)
}

public struct SettingsResponse: Codable, Equatable, Sendable {
    public let settings: UISettings
    public let displayCatalog: [DisplayField]
    public let claudeQuotaBridge: ClaudeQuotaBridge
    public let backups: BackupSummary
}

public enum AttentionAction: String, Codable, Equatable, Sendable {
    case approve
    case deny
    case passThrough = "pass_through"
    case ack
    case snooze
    case dismiss
}
