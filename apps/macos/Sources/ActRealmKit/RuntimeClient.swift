import Foundation
import Combine

public enum RuntimeClientError: Error, LocalizedError, Sendable {
    case notConnected
    case bootstrapFailed
    case missingSessionCookie
    case requestFailed(Int, code: String?, detail: String?)

    public var errorDescription: String? {
        code
    }

    public var code: String? {
        switch self {
        case .notConnected:
            "RUNTIME_NOT_CONNECTED"
        case .bootstrapFailed:
            "RUNTIME_AUTH_FAILED"
        case .missingSessionCookie:
            "RUNTIME_SESSION_MISSING"
        case .requestFailed(let status, let code, _):
            code ?? "HTTP_\(status)"
        }
    }
}

public struct RuntimeStatusSnapshot: Codable, Equatable, Sendable {
    public static let supportedSchemaVersion = 2

    public struct Service: Codable, Equatable, Sendable {
        public let status: String
        public let address: String?
    }

    public struct WebSocket: Codable, Equatable, Sendable {
        public let status: String
        public let connections: UInt64
    }

    public struct Hook: Codable, Equatable, Sendable {
        public let status: String
        public let name: String
        public let `private`: Bool
        public let lastEventAt: UInt64?
    }

    public struct Counts: Codable, Equatable, Sendable {
        public let active: UInt64?
        public let total: UInt64?
        public let pending: UInt64?
        public let waiters: UInt64?
    }

    public struct Projection: Codable, Equatable, Sendable {
        public let revision: UInt64
        public let revisionSource: String
        public let lastEventAt: UInt64?
        public let freshness: String
    }

    public struct Storage: Codable, Equatable, Sendable {
        public let status: String
        public let eventCount: UInt64
        public let schemaVersion: Int?
        public let expectedSchemaVersion: Int?
        public let integrity: String?
        public let checkedAt: UInt64?
    }

    public struct Collector: Codable, Equatable, Sendable {
        public let status: String
        public let source: String
        public let gitCheck: String?
        public let dataQuality: String?
        public let pendingBaselines: UInt64?
        public let inProgress: Bool
        public let historyComplete: Bool?
        public let consecutiveFailures: UInt64
        public let lastSuccessfulAt: UInt64?
    }

    public struct Collectors: Codable, Equatable, Sendable {
        public let review: Collector
        public let token: Collector
    }

    public struct Companion: Codable, Equatable, Sendable {
        public let status: String
        public let protocolVersion: UInt32
        public let registrations: UInt64
        public let scopes: [String]
    }

    public struct ConditionalFeature: Codable, Equatable, Sendable {
        public let status: String
        public let countsAsFault: Bool
        public let reason: String
    }

    public struct Conditional: Codable, Equatable, Sendable {
        public let claudeCowork: ConditionalFeature
    }

    public struct Restart: Codable, Equatable, Sendable {
        public let count: UInt64
        public let lastResult: String
    }

    public let schemaVersion: Int
    public let generatedAt: UInt64?
    public let instanceId: String
    public let pid: UInt32
    public let version: String
    public let commit: String?
    public let protocolVersion: UInt32?
    public let startedAt: UInt64
    public let uptimeMs: UInt64
    public let api: Service
    public let websocket: WebSocket
    public let hook: Hook
    public let sessions: Counts
    public let snapshot: Projection?
    public let attention: Counts
    public let storage: Storage
    public let collectors: Collectors?
    public let companion: Companion?
    public let conditional: Conditional?
    public let restart: Restart

    public var isSupported: Bool {
        schemaVersion == Self.supportedSchemaVersion
    }

    public static let preview = RuntimeStatusSnapshot(
        schemaVersion: 2,
        generatedAt: 1_800_000_000_000,
        instanceId: "019f0000-0000-7000-8000-000000000001",
        pid: 4242,
        version: "0.1.0",
        commit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        protocolVersion: 2,
        startedAt: 1_799_999_100_000,
        uptimeMs: 900_000,
        api: Service(status: "ready", address: "127.0.0.1:43111"),
        websocket: WebSocket(status: "ready", connections: 1),
        hook: Hook(
            status: "ready",
            name: "bridge.sock",
            private: true,
            lastEventAt: 1_799_999_999_000
        ),
        sessions: Counts(active: 2, total: 12, pending: nil, waiters: nil),
        snapshot: Projection(
            revision: 1_248,
            revisionSource: "runtime:sqlite_event_count",
            lastEventAt: 1_799_999_999_000,
            freshness: "live"
        ),
        attention: Counts(active: nil, total: nil, pending: 1, waiters: 1),
        storage: Storage(
            status: "ready",
            eventCount: 1_248,
            schemaVersion: 34,
            expectedSchemaVersion: 34,
            integrity: "ok",
            checkedAt: 1_800_000_000_000
        ),
        collectors: Collectors(
            review: Collector(
                status: "ready",
                source: "runtime:review_baseline_queue",
                gitCheck: "on_demand",
                dataQuality: nil,
                pendingBaselines: 0,
                inProgress: false,
                historyComplete: nil,
                consecutiveFailures: 0,
                lastSuccessfulAt: 1_800_000_000_000
            ),
            token: Collector(
                status: "ready",
                source: "runtime:canonical_session_ledger",
                gitCheck: nil,
                dataQuality: "verified",
                pendingBaselines: nil,
                inProgress: false,
                historyComplete: true,
                consecutiveFailures: 0,
                lastSuccessfulAt: 1_800_000_000_000
            )
        ),
        companion: Companion(
            status: "ready",
            protocolVersion: 2,
            registrations: 1,
            scopes: ["snapshot.read", "session.jump"]
        ),
        conditional: Conditional(
            claudeCowork: ConditionalFeature(
                status: "unsupported",
                countsAsFault: false,
                reason: "no_verified_event_source"
            )
        ),
        restart: Restart(count: 1, lastResult: "recovered")
    )
}

public struct JumpResponse: Codable, Equatable, Sendable {
    public let success: Bool
    public let capability: String
    public let label: String
    public let labelMessage: RuntimeMessage?
}



public struct RuntimeTimelineEvent: Codable, Equatable, Sendable {
    public let schemaVersion: UInt16?
    public let eventId: String
    public let provider: String
    public let kind: String
    public let toolName: String?
    public let toolCategory: String?
    public let toolTarget: String?
    public let toolCallId: String?
    public let sourceVersion: String?
    public let validationStatus: String?
    public let phase: String?
    public let status: String?
    public let confidence: String?
    public let riskLevel: String?
    public let planStepCount: UInt32?
    public let turnId: String?
    public let outboxId: String?
    public let occurredAt: UInt64
    public let ingestSequence: UInt64
    public let contextAvailability: String

    public init(
        schemaVersion: UInt16? = nil,
        eventId: String,
        provider: String,
        kind: String,
        toolName: String? = nil,
        toolCategory: String? = nil,
        toolTarget: String? = nil,
        toolCallId: String? = nil,
        sourceVersion: String? = nil,
        validationStatus: String? = nil,
        phase: String? = nil,
        status: String? = nil,
        confidence: String? = nil,
        riskLevel: String? = nil,
        planStepCount: UInt32? = nil,
        turnId: String? = nil,
        outboxId: String? = nil,
        occurredAt: UInt64,
        ingestSequence: UInt64,
        contextAvailability: String
    ) {
        self.schemaVersion = schemaVersion
        self.eventId = eventId
        self.provider = provider
        self.kind = kind
        self.toolName = toolName
        self.toolCategory = toolCategory
        self.toolTarget = toolTarget
        self.toolCallId = toolCallId
        self.sourceVersion = sourceVersion
        self.validationStatus = validationStatus
        self.phase = phase
        self.status = status
        self.confidence = confidence
        self.riskLevel = riskLevel
        self.planStepCount = planStepCount
        self.turnId = turnId
        self.outboxId = outboxId
        self.occurredAt = occurredAt
        self.ingestSequence = ingestSequence
        self.contextAvailability = contextAvailability
    }
}

public struct RuntimeTimelinePage: Codable, Equatable, Sendable {
    public let events: [RuntimeTimelineEvent]
    public let nextAfterIngestSequence: UInt64?
    public let hasMore: Bool
}

public struct RuntimeReviewOutcome: Codable, Equatable, Sendable {
    public let state: String
    public let source: String
    public let verification: String
    public let observedAt: UInt64
}

public struct RuntimeReviewRepository: Codable, Equatable, Sendable {
    public let state: String
    public let baselineState: String?
    public let baselineCapturedAt: UInt64?
    public let baselineHead: String?
    public let commitCount: UInt64?
    public let branch: String?
    public let head: String?
    public let worktreeKind: String?
    public let dirty: Bool?
    public let changedFiles: UInt64?
    public let stagedFiles: UInt64?
    public let unstagedFiles: UInt64?
    public let untrackedFiles: UInt64?
    public let insertions: UInt64?
    public let deletions: UInt64?
    public let binaryFiles: UInt64?
    public let attribution: String
    public let attributionReason: String
}

public struct RuntimeReviewValidationRun: Codable, Equatable, Identifiable, Sendable {
    public let id: String
    public let kind: String
    public let state: String
    public let source: String
    public let toolName: String?
    public let observedAt: UInt64
}

public struct RuntimeReviewLastAction: Codable, Equatable, Sendable {
    public let kind: String
    public let state: String
    public let toolName: String?
    public let observedAt: UInt64
}

public struct RuntimeTaskReviewSnapshot: Codable, Equatable, Sendable {
    public static let supportedSchemaVersion: UInt16 = 1

    public let schemaVersion: UInt16
    public let sessionId: String
    public let provider: String
    public let projectLabel: String?
    public let generatedAt: UInt64
    public let turnStartedAt: UInt64?
    public let turnEndedAt: UInt64?
    public let outcome: RuntimeReviewOutcome
    public let repository: RuntimeReviewRepository
    public let validations: [RuntimeReviewValidationRun]
    public let lastMeaningfulAction: RuntimeReviewLastAction?
    public let limitations: [String]
}

public struct RuntimeReviewDiffFile: Codable, Equatable, Identifiable, Sendable {
    public let path: String
    public let state: String
    public var id: String { path }
}

public struct RuntimeReviewDiffPatch: Codable, Equatable, Sendable {
    public let path: String
    public let patch: String
    public let truncated: Bool
}

public struct RuntimeReviewDiffResponse: Codable, Equatable, Sendable {
    public let schemaVersion: UInt16
    public let sessionId: String
    public let base: String?
    public let attribution: String
    public let files: [RuntimeReviewDiffFile]
    public let selected: RuntimeReviewDiffPatch?
    public let limitation: String?
}

public struct RuntimeCheckpointRepository: Codable, Equatable, Sendable {
    public let state: String
    public let branch: String?
    public let head: String?
    public let worktreeKind: String?
    public let dirty: Bool?
    public let changedFiles: UInt64?
    public let stagedFiles: UInt64?
    public let unstagedFiles: UInt64?
    public let untrackedFiles: UInt64?
    public let gitSnapshot: Bool
    public let gitObject: String?
}

public struct RuntimeTaskCheckpoint: Codable, Equatable, Identifiable, Sendable {
    public let schemaVersion: UInt16
    public let id: String
    public let sessionId: String
    public let turnId: String
    public let label: String?
    public let kind: String
    public let provider: String
    public let providerResumeCapability: String
    public let createdAt: UInt64
    public let repository: RuntimeCheckpointRepository
    public let validations: [RuntimeReviewValidationRun]
    public let validationIsHistorical: Bool
    public let limitations: [String]
}

public struct RuntimeTaskCheckpointList: Codable, Equatable, Sendable {
    public let schemaVersion: UInt16
    public let sessionId: String
    public let checkpoints: [RuntimeTaskCheckpoint]
}

public struct RuntimeCheckpointPreflight: Codable, Equatable, Sendable {
    public let schemaVersion: UInt16
    public let checkpointId: String
    public let action: String
    public let allowed: Bool
    public let blockers: [String]
    public let warnings: [String]
    public let currentBranch: String?
    public let currentHead: String?
    public let currentDirty: Bool?
    public let currentChangedFiles: UInt64?
    public let validationIsHistorical: Bool
}

public struct RuntimeCheckpointActionResponse: Codable, Equatable, Sendable {
    public let success: Bool
    public let checkpointId: String
    public let action: String
    public let validationState: String
}

public struct RuntimeTaskHistoryRecord: Codable, Equatable, Identifiable, Sendable {
    public let id: String
    public let provider: String
    public let project: String?
    public let title: String?
    public let model: String?
    public let status: String
    public let startedAt: UInt64
    public let lastEventAt: UInt64
    public let completedAt: UInt64?
    public let archivedAt: UInt64?
    public let archiveReason: String?
    public let branch: String?
    public let validationState: String?
    public let checkpointCount: UInt32
    public let securityEventCount: UInt32
    public let jumpCapability: String
    public let jumpLabel: String
}

public struct RuntimeTaskHistoryResponse: Codable, Equatable, Sendable {
    public static let supportedSchemaVersion: UInt16 = 1

    public let schemaVersion: UInt16
    public let generatedAt: UInt64
    public let tasks: [RuntimeTaskHistoryRecord]
}




/// An explicitly publishable projection. Runtime has already removed plan
/// detail and rejected unsafe text before this value reaches the app.

/// Talks to a single running `actrealm` backend: performs the one-time
/// bootstrap handshake, keeps the live snapshot up to date over WebSocket,
/// and exposes the same authenticated actions as the browser control surface.
@MainActor
public final class RuntimeClient: ObservableObject {
    public enum ConnectionState: Equatable, Sendable {
        case idle
        case connecting
        case live
        case error(String)
    }

    @Published public private(set) var connectionState: ConnectionState = .idle
    @Published public private(set) var snapshot: Snapshot = .empty
    let snapshotUpdates = PassthroughSubject<SnapshotUpdate, Never>()

    private var session: URLSession
    private var baseURL: URL?
    private var sessionCookie: String?
    private var csrfToken: String?
    private var webSocketTask: URLSessionWebSocketTask?
    private var streamTask: Task<Void, Never>?
    private var connectionGeneration = 0

    public init() {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        self.session = URLSession(configuration: configuration)
    }

    public func connect(baseURL: URL, token: String) async {
        disconnect()
        let generation = connectionGeneration
        self.baseURL = baseURL
        connectionState = .connecting
        do {
            try await bootstrap(baseURL: baseURL, token: token)
            try await refreshSnapshotThrowing()
            guard generation == connectionGeneration else { return }
            startStreaming()
        } catch {
            if generation == connectionGeneration { connectionState = .error(error.localizedDescription) }
        }
    }

    public func connect(credentials: LocalRuntimeCredentials) async {
        disconnect()
        let generation = connectionGeneration
        guard credentials.isSupported, let baseURL = credentials.baseURL, let cookie = credentials.sessionToken,
              let csrf = credentials.csrfToken, LocalRuntimeService.isSecret(cookie), LocalRuntimeService.isSecret(csrf) else {
            connectionState = .error(LocalRuntimeServiceError.incompatible.localizedDescription)
            return
        }
        self.session.invalidateAndCancel()
        self.session = LocalRuntimeService.makeSession(credentials: credentials)
        self.baseURL = baseURL
        sessionCookie = cookie
        csrfToken = csrf
        connectionState = .connecting
        do {
            try await refreshSnapshotThrowing()
            guard generation == connectionGeneration else { return }
            startStreaming()
        } catch { if generation == connectionGeneration { connectionState = .error(error.localizedDescription) } }
    }

    public func disconnect() {
        connectionGeneration += 1
        streamTask?.cancel()
        streamTask = nil
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
        sessionCookie = nil
        csrfToken = nil
        connectionState = .idle
    }

    public func refreshSnapshot() async {
        do {
            try await refreshSnapshotThrowing()
        } catch {
            connectionState = .error(error.localizedDescription)
        }
    }

    /// macOS may leave an apparently live URLSession WebSocket suspended
    /// across sleep. Recreate only the transport, retain the authenticated
    /// local session, and explicitly invalidate Runtime's monotonic quota
    /// cache so Claude OAuth is refreshed without waiting for a CLI event.
    public func recoverAfterSystemWake() async -> String? {
        guard baseURL != nil, sessionCookie != nil, csrfToken != nil else {
            return RuntimeClientError.notConnected.localizedDescription
        }
        streamTask?.cancel()
        streamTask = nil
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
        connectionState = .connecting
        do {
            _ = try await sendEmpty(
                "api/v1/quota/refresh",
                method: "POST",
                as: JSONValue.self
            )
            try await refreshSnapshotThrowing()
            startStreaming()
            return nil
        } catch {
            connectionState = .error(error.localizedDescription)
            return error.localizedDescription
        }
    }

    /// Explicit user recovery path for stale Provider quota. Runtime performs
    /// the authenticated collection; the client never reads OAuth credentials.
    struct QuotaRefreshReceipt: Decodable {
        let completed: Bool
        let claudeCapturedAt: UInt64?
    }

    public func requestQuotaRefresh() async -> String? {
        guard baseURL != nil, sessionCookie != nil, csrfToken != nil else {
            return RuntimeClientError.notConnected.localizedDescription
        }
        do {
            let receipt = try await sendEmpty(
                "api/v1/quota/refresh-now",
                method: "POST",
                as: QuotaRefreshReceipt.self
            )
            guard receipt.completed, receipt.claudeCapturedAt != nil else {
                return "CLAUDE_QUOTA_REFRESH_FAILED"
            }
            try await refreshSnapshotThrowing()
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    private func refreshSnapshotThrowing() async throws {
        let generation = connectionGeneration
        let next = try await get("api/v1/snapshot", as: Snapshot.self)
        guard generation == connectionGeneration else { throw CancellationError() }
        snapshot = next
        snapshotUpdates.send(SnapshotUpdate(snapshot: next))
    }

    // MARK: - Attention and sessions

    /// Returns nil only after Runtime accepted the command. Callers must not
    /// present a success confirmation before this method completes.
    public func send(
        action: AttentionAction,
        attentionId: String,
        requestId: UUID?
    ) async -> String? {
        let command = CommandRequest(attentionId: attentionId, requestId: requestId, action: action.rawValue)
        do {
            _ = try await sendCommand(command)
            try await refreshSnapshotThrowing()
            return nil
        } catch {
            connectionState = .error(error.localizedDescription)
            return error.localizedDescription
        }
    }

    public func dismissAttention(_ attention: AttentionRecord) async -> String? {
        if attention.kind == "question", let requestId = attention.requestId {
            return await answerQuestion(requestId: requestId, action: "native")
        }
        do {
            _ = try await sendCommand(CommandRequest(
                attentionId: attention.id,
                requestId: attention.requestId,
                action: AttentionAction.dismiss.rawValue
            ))
            try await refreshSnapshotThrowing()
            return nil
        } catch {
            return error.localizedDescription
        }
    }


    private func sendCommand(_ command: CommandRequest) async throws -> CommandResponse {
        try await sendJSON("api/v1/commands", method: "POST", body: command, as: CommandResponse.self)
    }

    public func undo(commandId: UUID) async {
        do {
            _ = try await sendEmpty(
                "api/v1/commands/\(commandId.uuidString)/undo",
                method: "POST",
                as: CommandResponse.self
            )
        } catch {
            connectionState = .error(error.localizedDescription)
        }
    }

    public func answerQuestion(
        requestId: UUID,
        action: String,
        answers: [String: JSONValue]? = nil
    ) async -> String? {
        struct Submission: Encodable {
            let action: String
            let answers: [String: JSONValue]?
        }
        do {
            _ = try await sendJSON(
                "api/v1/questions/\(requestId.uuidString)/answer",
                method: "POST",
                body: Submission(action: action, answers: answers),
                as: JSONValue.self
            )
            try await refreshSnapshotThrowing()
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    public func jumpSession(_ sessionId: String) async -> (JumpResponse?, String?) {
        do {
            let response = try await sendEmpty(
                "api/v1/sessions/\(sessionId)/jump",
                method: "POST",
                as: JumpResponse.self
            )
            return (response, nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    /// Reads the newest bounded, sanitized Runtime facts for a local task.
    /// Runtime never returns prompt text, tool input/output, commands, or file
    /// contents from this endpoint.
    public func sessionActivity(
        sessionId: String,
        limit: Int = 16,
        beforeIngestSequence: UInt64? = nil
    ) async throws -> RuntimeTimelinePage {
        var queryItems = [
            URLQueryItem(name: "latest", value: "true"),
            URLQueryItem(name: "currentTurn", value: "true"),
            URLQueryItem(
                name: "limit",
                value: String(min(max(limit, 1), 100))
            )
        ]
        if let beforeIngestSequence {
            queryItems.append(URLQueryItem(
                name: "beforeIngestSequence",
                value: String(beforeIngestSequence)
            ))
        }
        return try await get(
            "api/v1/sessions/\(sessionId)/timeline",
            queryItems: queryItems,
            as: RuntimeTimelinePage.self
        )
    }

    /// Reads an on-demand local Review. Git paths, Diff content, commands,
    /// prompts, and Provider payloads never leave the Runtime process.
    public func sessionReview(sessionId: String) async throws -> RuntimeTaskReviewSnapshot {
        try await get(
            "api/v1/sessions/\(sessionId)/review",
            queryItems: [],
            as: RuntimeTaskReviewSnapshot.self
        )
    }

    public func sessionReviewDiff(
        sessionId: String,
        path: String? = nil
    ) async throws -> RuntimeReviewDiffResponse {
        try await get(
            "api/v1/sessions/\(sessionId)/review/diff",
            queryItems: path.map { [URLQueryItem(name: "path", value: $0)] } ?? [],
            as: RuntimeReviewDiffResponse.self
        )
    }

    public func sessionCheckpoints(
        sessionId: String
    ) async throws -> [RuntimeTaskCheckpoint] {
        let response = try await get(
            "api/v1/sessions/\(sessionId)/checkpoints",
            queryItems: [],
            as: RuntimeTaskCheckpointList.self
        )
        return response.checkpoints
    }

    public func createCheckpoint(
        sessionId: String,
        kind: String,
        label: String? = nil
    ) async throws -> RuntimeTaskCheckpoint {
        struct Request: Encodable {
            let kind: String
            let label: String?
        }
        return try await sendJSON(
            "api/v1/sessions/\(sessionId)/checkpoints",
            method: "POST",
            body: Request(kind: kind, label: label),
            as: RuntimeTaskCheckpoint.self
        )
    }

    public func checkpointPreflight(
        checkpointId: String,
        action: String
    ) async throws -> RuntimeCheckpointPreflight {
        try await get(
            "api/v1/checkpoints/\(checkpointId)/preflight",
            queryItems: [URLQueryItem(name: "action", value: action)],
            as: RuntimeCheckpointPreflight.self
        )
    }

    public func applyCheckpointAction(
        checkpointId: String,
        action: String
    ) async throws -> RuntimeCheckpointActionResponse {
        struct Request: Encodable { let action: String }
        return try await sendJSON(
            "api/v1/checkpoints/\(checkpointId)/actions",
            method: "POST",
            body: Request(action: action),
            as: RuntimeCheckpointActionResponse.self
        )
    }

    public func deleteCheckpoint(checkpointId: String) async throws {
        _ = try await sendEmpty(
            "api/v1/checkpoints/\(checkpointId)",
            method: "DELETE",
            as: JSONValue.self
        )
    }

    /// Loads only bounded, sanitized task summaries. Review, Checkpoint and
    /// Diff data stay behind their existing on-demand endpoints.
    public func taskHistory(limit: Int = 300) async throws -> RuntimeTaskHistoryResponse {
        try await get(
            "api/v1/history",
            queryItems: [URLQueryItem(
                name: "limit",
                value: String(min(max(limit, 1), 500))
            )],
            as: RuntimeTaskHistoryResponse.self
        )
    }

    public func archiveTask(sessionId: String) async throws {
        _ = try await sendEmpty(
            "api/v1/sessions/\(sessionId)/archive",
            method: "POST",
            as: JSONValue.self
        )
    }

    public func deleteTaskHistory(sessionId: String) async throws {
        _ = try await sendEmpty(
            "api/v1/sessions/\(sessionId)/history",
            method: "DELETE",
            as: JSONValue.self
        )
        try await refreshSnapshotThrowing()
    }

    public func manageSession(_ sessionId: String) async -> String? {
        do {
            _ = try await sendJSON(
                "api/v1/sessions/\(sessionId)/manage",
                method: "POST",
                body: ["action": "attach"],
                as: JSONValue.self
            )
            try await refreshSnapshotThrowing()
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    // MARK: - Setup (Provider Hooks)

    public func fetchSetup() async -> SetupInfo? {
        try? await get("api/v1/setup", as: SetupInfo.self)
    }

    public func fetchRuntimeStatus() async throws -> RuntimeStatusSnapshot {
        try await get("api/v1/runtime/status", as: RuntimeStatusSnapshot.self)
    }

    /// Action: `install`, `repair`, or `uninstall`. Hook ownership and file
    /// safety stay in Rust; the native app only invokes the Runtime contract.
    public func changeSetup(
        provider: String,
        action: String,
        enhancedCodexActivity: Bool? = nil
    ) async -> (SetupInfo?, String?) {
        struct Request: Encodable {
            let provider: String
            let action: String
            let enhancedCodexActivity: Bool?
        }
        do {
            let response = try await sendJSON(
                "api/v1/setup",
                method: "POST",
                body: Request(
                    provider: provider,
                    action: action,
                    enhancedCodexActivity: enhancedCodexActivity
                ),
                as: SetupInfo.self
            )
            return (response, nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    // MARK: - Settings, quota bridge, and local data

    public func fetchSettings() async -> SettingsResponse? {
        try? await get("api/v1/settings", as: SettingsResponse.self)
    }

    public func updateSettings(_ settings: UISettings) async -> (SettingsResponse?, String?) {
        do {
            let response = try await sendJSON(
                "api/v1/settings",
                method: "PUT",
                body: settings,
                as: SettingsResponse.self
            )
            return (response, nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    public func changeClaudeQuotaBridge(action: String) async -> (SettingsResponse?, String?) {
        do {
            let response = try await sendJSON(
                "api/v1/quota/claude-bridge",
                method: "POST",
                body: ["action": action],
                as: SettingsResponse.self
            )
            return (response, nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    // MARK: - Local companion pairing

    public func createCompanionPairing(
        clientName: String = "Display Companion",
        allowControl: Bool
    ) async -> (CompanionPairingResponse?, String?) {
        struct Request: Encodable {
            let clientName: String
            let allowControl: Bool
        }
        do {
            let response = try await sendJSON(
                "api/v1/companions/pairing",
                method: "POST",
                body: Request(clientName: clientName, allowControl: allowControl),
                as: CompanionPairingResponse.self
            )
            return (response, nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    public func fetchCompanionConnections() async -> [CompanionConnection]? {
        try? await get(
            "api/v1/companions",
            as: CompanionConnectionsResponse.self
        ).connections
    }

    public func revokeCompanion(id: String) async -> String? {
        do {
            _ = try await sendEmpty(
                "api/v1/companions/\(id)",
                method: "DELETE",
                as: JSONValue.self
            )
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    public func exportData(metricsOnly: Bool) async -> (Data?, String?) {
        let path = metricsOnly ? "api/v1/metrics/export" : "api/v1/export"
        do {
            return (try await requestData(path: path), nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    public func exportTokenUsage(csv: Bool) async -> (Data?, String?) {
        let path = csv ? "api/v1/token-usage/export.csv" : "api/v1/token-usage/export"
        do {
            return (try await requestData(path: path), nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    public func clearData(confirmation: String) async -> String? {
        do {
            _ = try await sendJSON(
                "api/v1/data/clear",
                method: "POST",
                body: ["confirmation": confirmation],
                as: JSONValue.self
            )
            try await refreshSnapshotThrowing()
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    public func clearBackups(confirmation: String) async -> String? {
        do {
            _ = try await sendJSON(
                "api/v1/backups/clear",
                method: "POST",
                body: ["confirmation": confirmation],
                as: JSONValue.self
            )
            return nil
        } catch {
            return error.localizedDescription
        }
    }

    public func recordMetric(_ event: String) async {
        _ = try? await sendJSON(
            "api/v1/metrics",
            method: "POST",
            body: ["event": event],
            as: JSONValue.self
        )
    }

    // MARK: - Bootstrap

    private struct BootstrapResponse: Decodable {
        let csrfToken: String
    }

    private struct WebSocketTicketResponse: Decodable {
        let ticket: String
    }

    private struct APIError: Decodable {
        struct Payload: Decodable {
            let code: String
            let detail: String?
        }
        let error: Payload
    }

    private func bootstrap(baseURL: URL, token: String) async throws {
        var request = URLRequest(url: baseURL.appendingPathComponent("api/v1/bootstrap"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue(originValue(for: baseURL), forHTTPHeaderField: "Origin")
        request.httpBody = try JSONEncoder().encode(["token": token])
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw RuntimeClientError.bootstrapFailed
        }
        guard let cookie = Self.sessionCookieValue(from: http) else {
            throw RuntimeClientError.missingSessionCookie
        }
        let decoded = try JSONDecoder().decode(BootstrapResponse.self, from: data)
        sessionCookie = cookie
        csrfToken = decoded.csrfToken
    }

    // MARK: - WebSocket streaming

    private func startStreaming() {
        streamTask?.cancel()
        streamTask = Task { [weak self] in
            await self?.streamLoop()
        }
    }

    private func streamLoop() async {
        let generation = connectionGeneration
        var attempt = 0
        while !Task.isCancelled {
            guard let baseURL, let cookie = sessionCookie, csrfToken != nil else { return }
            do {
                let ticket = try await sendEmpty(
                    "api/v1/ws-ticket",
                    method: "POST",
                    as: WebSocketTicketResponse.self
                ).ticket
                guard let request = Self.webSocketRequest(
                    baseURL: baseURL,
                    cookie: cookie,
                    ticket: ticket
                ) else { return }
                let task = session.webSocketTask(with: request)
                webSocketTask = task
                task.resume()
                let streamID = UUID()
                var snapshotSequence: UInt64 = 0
                var receivedMessage = false
                while !Task.isCancelled {
                    let message = try await task.receive()
                    let receivedAtNs = PerformanceClock.now()
                    guard !Task.isCancelled, generation == connectionGeneration else { return }
                    if !receivedMessage {
                        receivedMessage = true
                        connectionState = .live
                        attempt = 0
                    }
                    switch message {
                    case .string(let text):
                        if handleSocketText(text, streamID: streamID,
                            sequence: snapshotSequence + 1, receivedAtNs: receivedAtNs) {
                            snapshotSequence += 1
                        }
                    case .data(let data):
                        if let text = String(data: data, encoding: .utf8),
                           handleSocketText(text, streamID: streamID,
                            sequence: snapshotSequence + 1, receivedAtNs: receivedAtNs) {
                            snapshotSequence += 1
                        }
                    @unknown default: break
                    }
                }
            } catch {
                // Reconnect below. The last truthful snapshot stays visible.
            }
            if Task.isCancelled { return }

            connectionState = .connecting
            attempt += 1
            let delaySeconds = min(pow(2.0, Double(attempt)), 30)
            try? await Task.sleep(for: .seconds(delaySeconds))
        }
    }

    nonisolated static func webSocketRequest(
        baseURL: URL,
        cookie: String,
        ticket: String
    ) -> URLRequest? {
        guard var components = URLComponents(
            url: baseURL.appendingPathComponent("api/v1/ws"),
            resolvingAgainstBaseURL: false
        ) else { return nil }
        components.scheme = baseURL.scheme == "https" ? "wss" : "ws"
        components.query = nil
        components.fragment = nil
        guard let wsURL = components.url else { return nil }

        var request = URLRequest(url: wsURL)
        request.setValue("actrealm_session=\(cookie)", forHTTPHeaderField: "Cookie")
        var origin = baseURL.absoluteString
        if origin.hasSuffix("/") { origin.removeLast() }
        request.setValue(origin, forHTTPHeaderField: "Origin")
        request.setValue("actrealm.\(ticket)", forHTTPHeaderField: "Sec-WebSocket-Protocol")
        return request
    }

    @discardableResult
    private func handleSocketText(_ text: String, streamID: UUID,
        sequence: UInt64, receivedAtNs: UInt64) -> Bool {
        guard let data = text.data(using: .utf8),
              let envelope = try? JSONDecoder().decode(SnapshotEnvelope.self, from: data),
              envelope.type == "snapshot"
        else { return false }
        snapshot = envelope.snapshot
        snapshotUpdates.send(SnapshotUpdate(snapshot: envelope.snapshot,
            timing: SnapshotReceiveTiming(streamID: streamID, sequence: sequence,
                receivedAtNs: receivedAtNs, delivery: envelope.deliveryTiming)))
        return true
    }

    // MARK: - Request helpers

    private func get<Response: Decodable>(
        _ path: String,
        queryItems: [URLQueryItem] = [],
        as type: Response.Type
    ) async throws -> Response {
        let data = try await requestData(path: path, queryItems: queryItems)
        return try JSONDecoder().decode(type, from: data)
    }

    private func sendJSON<Body: Encodable, Response: Decodable>(
        _ path: String,
        method: String,
        body: Body,
        as type: Response.Type
    ) async throws -> Response {
        let data = try await requestData(
            path: path,
            method: method,
            mutating: true,
            body: try JSONEncoder().encode(body)
        )
        return try JSONDecoder().decode(type, from: data)
    }

    private func sendEmpty<Response: Decodable>(
        _ path: String,
        method: String,
        as type: Response.Type
    ) async throws -> Response {
        let data = try await requestData(path: path, method: method, mutating: true, body: nil)
        return try JSONDecoder().decode(type, from: data)
    }

    private func requestData(
        path: String,
        queryItems: [URLQueryItem] = [],
        method: String = "GET",
        mutating: Bool = false,
        body: Data? = nil
    ) async throws -> Data {
        var request = try authorizedRequest(
            path: path,
            queryItems: queryItems,
            method: method,
            mutating: mutating
        )
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = body
        }
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            let status = (response as? HTTPURLResponse)?.statusCode ?? -1
            let payload = try? JSONDecoder().decode(APIError.self, from: data)
            throw RuntimeClientError.requestFailed(
                status,
                code: payload?.error.code,
                detail: payload?.error.detail
            )
        }
        return data
    }

    private func authorizedRequest(
        path: String,
        queryItems: [URLQueryItem] = [],
        method: String = "GET",
        mutating: Bool = false
    ) throws -> URLRequest {
        guard let baseURL, let cookie = sessionCookie else { throw RuntimeClientError.notConnected }
        guard var components = URLComponents(
            url: baseURL.appendingPathComponent(path),
            resolvingAgainstBaseURL: false
        ) else {
            throw RuntimeClientError.notConnected
        }
        components.queryItems = queryItems.isEmpty ? nil : queryItems
        guard let url = components.url else {
            throw RuntimeClientError.notConnected
        }
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("actrealm_session=\(cookie)", forHTTPHeaderField: "Cookie")
        if mutating {
            guard let csrfToken else { throw RuntimeClientError.notConnected }
            request.setValue(originValue(for: baseURL), forHTTPHeaderField: "Origin")
            request.setValue(csrfToken, forHTTPHeaderField: "x-actrealm-csrf")
        }
        return request
    }

    private func originValue(for baseURL: URL) -> String {
        var value = baseURL.absoluteString
        if value.hasSuffix("/") { value.removeLast() }
        return value
    }

    private static func sessionCookieValue(from response: HTTPURLResponse) -> String? {
        for (key, value) in response.allHeaderFields {
            guard let keyString = key as? String,
                  keyString.caseInsensitiveCompare("Set-Cookie") == .orderedSame,
                  let valueString = value as? String
            else { continue }
            for part in valueString.split(separator: ";") {
                let trimmed = part.trimmingCharacters(in: .whitespaces)
                if trimmed.hasPrefix("actrealm_session=") {
                    return String(trimmed.dropFirst("actrealm_session=".count))
                }
            }
        }
        return nil
    }
}
