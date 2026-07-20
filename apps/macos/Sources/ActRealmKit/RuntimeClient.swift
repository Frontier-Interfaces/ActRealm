import Foundation

public enum RuntimeClientError: Error, Sendable {
    case notConnected
    case bootstrapFailed
    case missingSessionCookie
    case requestFailed(Int)
}

/// Talks to a single running `actrealm` backend: performs the one-time
/// bootstrap handshake, keeps the live snapshot up to date over the
/// WebSocket feed, and sends attention-queue commands.
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
        self.session = URLSession(configuration: configuration)
    }

    public func connect(baseURL: URL, token: String) async {
        self.baseURL = baseURL
        connectionState = .connecting
        do {
            try await bootstrap(baseURL: baseURL, token: token)
            await refreshSnapshot()
            startStreaming()
        } catch {
            connectionState = .error(String(describing: error))
        }
    }

    public func disconnect() {
        streamTask?.cancel()
        streamTask = nil
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
        connectionState = .idle
    }

    public func refreshSnapshot() async {
        do {
            let request = try authorizedRequest(path: "api/v1/snapshot")
            let (data, response) = try await session.data(for: request)
            try Self.requireOK(response)
            snapshot = try JSONDecoder().decode(Snapshot.self, from: data)
        } catch {
            connectionState = .error(String(describing: error))
        }
    }

    public func send(action: AttentionAction, attentionId: String, requestId: UUID?) async {
        let command = CommandRequest(attentionId: attentionId, requestId: requestId, action: action.rawValue)
        do {
            var request = try authorizedRequest(path: "api/v1/commands", method: "POST", mutating: true)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONEncoder().encode(command)
            let (_, response) = try await session.data(for: request)
            try Self.requireOK(response)
        } catch {
            connectionState = .error(String(describing: error))
        }
    }

    public func undo(commandId: UUID) async {
        do {
            let request = try authorizedRequest(
                path: "api/v1/commands/\(commandId.uuidString)/undo",
                method: "POST",
                mutating: true
            )
            let (_, response) = try await session.data(for: request)
            try Self.requireOK(response)
        } catch {
            connectionState = .error(String(describing: error))
        }
    }

    // MARK: - Setup (provider hooks)

    public func fetchSetup() async -> SetupInfo? {
        do {
            let request = try authorizedRequest(path: "api/v1/setup")
            let (data, response) = try await session.data(for: request)
            try Self.requireOK(response)
            return try JSONDecoder().decode(SetupInfo.self, from: data)
        } catch {
            return nil
        }
    }

    /// action: "install" | "repair" | "uninstall". Returns the refreshed
    /// setup state, or nil with the server's error code on failure.
    public func changeSetup(provider: String, action: String) async -> (SetupInfo?, String?) {
        do {
            var request = try authorizedRequest(path: "api/v1/setup", method: "POST", mutating: true)
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONEncoder().encode(["provider": provider, "action": action])
            let (data, response) = try await session.data(for: request)
            if let http = response as? HTTPURLResponse, !(200...299).contains(http.statusCode) {
                let detail = (try? JSONDecoder().decode(APIError.self, from: data))?.error.detail
                return (nil, detail ?? "请求失败（\(http.statusCode)）")
            }
            return (try JSONDecoder().decode(SetupInfo.self, from: data), nil)
        } catch {
            return (nil, error.localizedDescription)
        }
    }

    private struct APIError: Decodable {
        struct Payload: Decodable {
            let code: String
            let detail: String?
        }
        let error: Payload
    }

    // MARK: - Bootstrap

    private struct BootstrapResponse: Decodable {
        let csrfToken: String
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
                    case .string(let text):
                        handleSocketText(text)
                    case .data(let data):
                        if let text = String(data: data, encoding: .utf8) {
                            handleSocketText(text)
                        }
                    @unknown default:
                        break
                    }
                }
            } catch {
                // fall through to reconnect below
            }
            if Task.isCancelled { return }

            connectionState = .connecting
            attempt += 1
            let delaySeconds = min(pow(2.0, Double(attempt)), 30)
            try? await Task.sleep(nanoseconds: UInt64(delaySeconds * 1_000_000_000))
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

    private func authorizedRequest(path: String, method: String = "GET", mutating: Bool = false) throws -> URLRequest {
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
        if value.hasSuffix("/") {
            value.removeLast()
        }
        return value
    }

    private static func requireOK(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            let code = (response as? HTTPURLResponse)?.statusCode ?? -1
            throw RuntimeClientError.requestFailed(code)
        }
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
