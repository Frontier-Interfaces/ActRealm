import Foundation

public enum RuntimeClientError: Error, LocalizedError, Sendable {
    case notConnected
    case bootstrapFailed
    case missingSessionCookie
    case requestFailed(Int, code: String?, detail: String?)

    public var errorDescription: String? {
        switch self {
        case .notConnected: "Runtime 尚未连接"
        case .bootstrapFailed: "Runtime 身份验证失败"
        case .missingSessionCookie: "Runtime 没有返回本机会话"
        case .requestFailed(let status, let code, let detail):
            detail ?? code ?? "请求失败（\(status)）"
        }
    }

    public var code: String? {
        if case .requestFailed(_, let code, _) = self { return code }
        return nil
    }
}

public struct JumpResponse: Codable, Equatable, Sendable {
    public let success: Bool
    public let capability: String
    public let label: String
}

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

    private let session: URLSession
    private var baseURL: URL?
    private var sessionCookie: String?
    private var csrfToken: String?
    private var webSocketTask: URLSessionWebSocketTask?
    private var streamTask: Task<Void, Never>?

    public init() {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        self.session = URLSession(configuration: configuration)
    }

    public func connect(baseURL: URL, token: String) async {
        self.baseURL = baseURL
        connectionState = .connecting
        do {
            try await bootstrap(baseURL: baseURL, token: token)
            try await refreshSnapshotThrowing()
            startStreaming()
        } catch {
            connectionState = .error(error.localizedDescription)
        }
    }

    public func disconnect() {
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

    private func refreshSnapshotThrowing() async throws {
        snapshot = try await get("api/v1/snapshot", as: Snapshot.self)
    }

    // MARK: - Attention and sessions

    public func send(action: AttentionAction, attentionId: String, requestId: UUID?) async {
        let command = CommandRequest(attentionId: attentionId, requestId: requestId, action: action.rawValue)
        do {
            _ = try await sendCommand(command)
        } catch {
            connectionState = .error(error.localizedDescription)
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

    public func exportData(metricsOnly: Bool) async -> (Data?, String?) {
        let path = metricsOnly ? "api/v1/metrics/export" : "api/v1/export"
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
        var attempt = 0
        while !Task.isCancelled {
            guard let baseURL, let cookie = sessionCookie, let csrfToken else { return }
            guard var components = URLComponents(
                url: baseURL.appendingPathComponent("api/v1/ws"),
                resolvingAgainstBaseURL: false
            ) else { return }
            components.scheme = baseURL.scheme == "https" ? "wss" : "ws"
            components.queryItems = [URLQueryItem(name: "csrf", value: csrfToken)]
            guard let wsURL = components.url else { return }

            var request = URLRequest(url: wsURL)
            request.setValue("actrealm_session=\(cookie)", forHTTPHeaderField: "Cookie")
            request.setValue(originValue(for: baseURL), forHTTPHeaderField: "Origin")
            let task = session.webSocketTask(with: request)
            webSocketTask = task
            task.resume()
            connectionState = .live
            attempt = 0

            do {
                while !Task.isCancelled {
                    let message = try await task.receive()
                    switch message {
                    case .string(let text): handleSocketText(text)
                    case .data(let data):
                        if let text = String(data: data, encoding: .utf8) { handleSocketText(text) }
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

    private func handleSocketText(_ text: String) {
        guard let data = text.data(using: .utf8),
              let envelope = try? JSONDecoder().decode(SnapshotEnvelope.self, from: data),
              envelope.type == "snapshot"
        else { return }
        snapshot = envelope.snapshot
    }

    // MARK: - Request helpers

    private func get<Response: Decodable>(_ path: String, as type: Response.Type) async throws -> Response {
        let data = try await requestData(path: path)
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
        method: String = "GET",
        mutating: Bool = false,
        body: Data? = nil
    ) async throws -> Data {
        var request = try authorizedRequest(path: path, method: method, mutating: mutating)
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
        method: String = "GET",
        mutating: Bool = false
    ) throws -> URLRequest {
        guard let baseURL, let cookie = sessionCookie else { throw RuntimeClientError.notConnected }
        var request = URLRequest(url: baseURL.appendingPathComponent(path))
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
