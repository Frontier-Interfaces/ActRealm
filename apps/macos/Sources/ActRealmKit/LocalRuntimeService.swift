import Darwin
import Foundation
import Security
import CryptoKit

public struct LocalRuntimeCredentials: Codable, Sendable {
    public let schemaVersion: Int
    public let protocolVersion: Int
    public let endpoint: String
    public let instanceId: String
    public let sessionToken: String?
    public let csrfToken: String?
    public let token: String?
    public let companionId: String?
    public let scopes: [String]?
    public let discoveryPath: String?
    public let certificateSha256: String?

    public var isSupported: Bool {
        schemaVersion == 1 && protocolVersion >= 7 && baseURL != nil && UUID(uuidString: instanceId) != nil
            && certificateSha256.map(LocalRuntimeService.isSecret) == true
    }

    public var baseURL: URL? {
        guard let url = URL(string: endpoint), url.scheme == "https", url.host == "127.0.0.1",
              let port = url.port, (1...65535).contains(port), url.user == nil,
              url.password == nil, url.query == nil, url.fragment == nil,
              url.path.isEmpty || url.path == "/" else { return nil }
        return url
    }
}

enum LocalRuntimeServiceError: LocalizedError {
    case unavailable
    case untrusted
    case accessRevoked
    case incompatible
    case serviceFailed(String)

    var errorDescription: String? {
        switch self {
        case .unavailable: "本机 Runtime 不可用，请重新连接。"
        case .untrusted: "无法验证本机应用或 Runtime 的签名。"
        case .accessRevoked: "本机 Agent 访问已停用，请在当前应用中主动启用。"
        case .incompatible: "本机 Runtime 版本需要更新。"
        case .serviceFailed(let message):
            message.lowercased().contains("older runtime")
                ? "旧版 Runtime 仍在运行，请退出旧版应用后更新服务。"
                : "无法启动本机 Runtime，请检查后台项目设置和诊断日志。"
        }
    }

    var diagnosticDescription: String {
        if case .serviceFailed(let message) = self { return message }
        return errorDescription ?? "Local Runtime error"
    }
}

/// Wire-compatible native transport, shared in source form with Display.
/// Secrets stay in memory or the caller's own Keychain entry, never in CLI output.
enum LocalRuntimeService {
    static let runtimeIdentifier = "com.frontierinterfaces.actrealm.runtime"
    static let minimumProtocol = 7

    static func isSecret(_ value: String) -> Bool {
        value.utf8.count == 64 && value.utf8.allSatisfy {
            (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0)
        }
    }

    static var socketURL: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".actrealm/run/native-clients.sock")
    }

    static func connect(helper: URL, previousToken: String? = nil, enableAccess: Bool = false, restart: Bool = false) throws -> LocalRuntimeCredentials {
        let team = try currentTeam()
        try verifyHelper(helper, team: team)
        try ensureService(helper: helper, restart: restart)
        return try enroll(socket: socketURL, team: team, previousToken: previousToken, enableAccess: enableAccess)
    }

    static func enroll(socket: URL, team: String, previousToken: String?, enableAccess: Bool) throws -> LocalRuntimeCredentials {
        if let previousToken, !isSecret(previousToken), !enableAccess {
            throw LocalRuntimeServiceError.accessRevoked
        }
        try verifyPrivateSocket(socket)
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw LocalRuntimeServiceError.unavailable }
        defer { Darwin.close(fd) }
        guard fcntl(fd, F_SETFD, FD_CLOEXEC) == 0 else { throw LocalRuntimeServiceError.unavailable }
        var timeout = timeval(tv_sec: 5, tv_usec: 0)
        var one: Int32 = 1
        guard setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size)) == 0,
              setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size)) == 0,
              setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size)) == 0
        else { throw LocalRuntimeServiceError.unavailable }
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        let path = Array(socket.path.utf8) + [0]
        guard path.count <= MemoryLayout.size(ofValue: address.sun_path) else { throw LocalRuntimeServiceError.unavailable }
        withUnsafeMutableBytes(of: &address.sun_path) { target in target.copyBytes(from: path) }
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else { throw LocalRuntimeServiceError.unavailable }
        // Verify the service before sending even an old migration credential.
        try verifyPeer(fd: fd, team: team)
        var request: [String: Any] = ["schemaVersion": 1, "enableAccess": enableAccess]
        if let previousToken, isSecret(previousToken) { request["previousToken"] = previousToken }
        var data = try JSONSerialization.data(withJSONObject: request)
        data.append(10)
        try data.withUnsafeBytes { bytes in
            var offset = 0
            while offset < bytes.count {
                let written = Darwin.write(fd, bytes.baseAddress!.advanced(by: offset), bytes.count - offset)
                if written < 0, errno == EINTR { continue }
                guard written > 0 else { throw LocalRuntimeServiceError.unavailable }
                offset += written
            }
        }
        var response = Data()
        var buffer = [UInt8](repeating: 0, count: 2048)
        while !response.contains(10) {
            let count = Darwin.read(fd, &buffer, buffer.count)
            if count < 0, errno == EINTR { continue }
            guard count > 0, response.count + count <= 32 * 1024 else { throw LocalRuntimeServiceError.unavailable }
            response.append(contentsOf: buffer.prefix(count))
        }
        if let error = (try JSONSerialization.jsonObject(with: response) as? [String: Any])?["error"] as? String {
            switch error {
            case "accessRevoked": throw LocalRuntimeServiceError.accessRevoked
            case "untrustedClient": throw LocalRuntimeServiceError.untrusted
            default: throw LocalRuntimeServiceError.unavailable
            }
        }
        let credentials = try JSONDecoder().decode(LocalRuntimeCredentials.self, from: response)
        guard credentials.isSupported else { throw LocalRuntimeServiceError.incompatible }
        return credentials
    }

    static func isCurrent(_ credentials: LocalRuntimeCredentials) async -> Bool {
        guard let url = credentials.baseURL else { return false }
        let discovery = socketURL.deletingLastPathComponent().appendingPathComponent("companion-endpoint.json")
        guard let data = try? privateData(discovery),
              let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              value["instanceId"] as? String == credentials.instanceId else { return false }
        let session = makeSession(credentials: credentials, timeout: 2)
        defer { session.invalidateAndCancel() }
        do {
            guard let cookie = credentials.sessionToken else { return false }
            var request = URLRequest(url: url.appendingPathComponent("api/v1/native/session"))
            request.setValue("actrealm_session=\(cookie)", forHTTPHeaderField: "Cookie")
            let (data, response) = try await session.data(for: request)
            guard (response as? HTTPURLResponse)?.statusCode == 200,
                  let body = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  body["ok"] as? Bool == true,
                  body["instanceId"] as? String == credentials.instanceId,
                  (body["protocolVersion"] as? Int ?? 0) >= minimumProtocol else { return false }
            return true
        } catch { return false }
    }

    static func makeSession(credentials: LocalRuntimeCredentials, timeout: TimeInterval = 30) -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCredentialStorage = nil
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = timeout
        configuration.connectionProxyDictionary = ["HTTPEnable": 0, "HTTPSEnable": 0, "SOCKSEnable": 0]
        return URLSession(configuration: configuration,
            delegate: NativeRuntimeTrustDelegate(fingerprint: credentials.certificateSha256 ?? ""), delegateQueue: nil)
    }

    private static func ensureService(helper: URL, restart: Bool) throws {
        let process = Process()
        process.executableURL = helper
        process.arguments = ["service", restart ? "restart" : "ensure"]
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        let errors = Pipe()
        process.standardError = errors
        try process.run()
        let deadline = Date().addingTimeInterval(50)
        while process.isRunning, Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        if process.isRunning {
            process.terminate()
            let stop = Date().addingTimeInterval(1)
            while process.isRunning, Date() < stop { Thread.sleep(forTimeInterval: 0.025) }
            if process.isRunning { kill(process.processIdentifier, SIGKILL) }
            throw LocalRuntimeServiceError.unavailable
        }
        guard process.terminationStatus == 0 else {
            let text = String(decoding: errors.fileHandleForReading.readDataToEndOfFile().prefix(2048), as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            throw LocalRuntimeServiceError.serviceFailed(text)
        }
    }

    private static func verifyPrivateSocket(_ url: URL) throws {
        for directory in [url.deletingLastPathComponent(), url.deletingLastPathComponent().deletingLastPathComponent()] {
            var info = stat()
            guard lstat(directory.path, &info) == 0, info.st_mode & S_IFMT == S_IFDIR,
                  info.st_uid == getuid(), info.st_mode & 0o077 == 0 else { throw LocalRuntimeServiceError.untrusted }
        }
        var info = stat()
        guard lstat(url.path, &info) == 0, info.st_mode & S_IFMT == S_IFSOCK,
              info.st_uid == getuid(), info.st_mode & 0o077 == 0 else { throw LocalRuntimeServiceError.unavailable }
    }

    private static func privateData(_ url: URL) throws -> Data {
        let fd = open(url.path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC)
        guard fd >= 0 else { throw LocalRuntimeServiceError.unavailable }
        defer { close(fd) }
        var info = stat()
        guard fstat(fd, &info) == 0, info.st_mode & S_IFMT == S_IFREG, info.st_uid == getuid(),
              info.st_mode & 0o077 == 0, info.st_size <= 8192 else { throw LocalRuntimeServiceError.untrusted }
        var bytes = [UInt8](repeating: 0, count: 8193)
        let count = read(fd, &bytes, bytes.count)
        guard count >= 0, count <= 8192 else { throw LocalRuntimeServiceError.unavailable }
        return Data(bytes.prefix(count))
    }

    private static func currentTeam() throws -> String {
        var code: SecCode?
        var staticCode: SecStaticCode?
        var info: CFDictionary?
        guard SecCodeCopySelf([], &code) == errSecSuccess, let code,
              SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess, let staticCode,
              SecCodeCopySigningInformation(staticCode, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
              let team = (info as? [String: Any])?[kSecCodeInfoTeamIdentifier as String] as? String,
              !team.isEmpty, team.count <= 32,
              team.utf8.allSatisfy({ ($0 >= 48 && $0 <= 57) || ($0 >= 65 && $0 <= 90) || ($0 >= 97 && $0 <= 122) })
        else { throw LocalRuntimeServiceError.untrusted }
        return team
    }

    private static func requirement(team: String) throws -> SecRequirement {
        let text = "anchor apple generic and certificate leaf[subject.OU] = \"\(team)\" and identifier \"\(runtimeIdentifier)\""
        var requirement: SecRequirement?
        guard SecRequirementCreateWithString(text as CFString, [], &requirement) == errSecSuccess,
              let requirement else { throw LocalRuntimeServiceError.untrusted }
        return requirement
    }

    private static func verifyHelper(_ url: URL, team: String) throws {
        var info = stat()
        guard lstat(url.path, &info) == 0, info.st_mode & S_IFMT == S_IFREG,
              info.st_mode & 0o022 == 0, info.st_mode & 0o111 != 0 else { throw LocalRuntimeServiceError.untrusted }
        var code: SecStaticCode?
        guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess, let code,
              SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate), try requirement(team: team)) == errSecSuccess
        else { throw LocalRuntimeServiceError.untrusted }
        try verifyHardened(code)
    }

    private static func verifyPeer(fd: Int32, team: String) throws {
        var uid: uid_t = 0
        var gid: gid_t = 0
        guard getpeereid(fd, &uid, &gid) == 0, uid == getuid() else { throw LocalRuntimeServiceError.untrusted }
        var audit = [UInt32](repeating: 0, count: 8)
        var length = socklen_t(audit.count * MemoryLayout<UInt32>.size)
        let expected = length
        guard getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &audit, &length) == 0, length == expected else { throw LocalRuntimeServiceError.untrusted }
        let data = audit.withUnsafeBytes { Data($0) }
        let attributes = [kSecGuestAttributeAudit as String: data] as CFDictionary
        var code: SecCode?
        guard SecCodeCopyGuestWithAttributes(nil, attributes, [], &code) == errSecSuccess, let code,
              SecCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate), try requirement(team: team)) == errSecSuccess
        else { throw LocalRuntimeServiceError.untrusted }
        var staticCode: SecStaticCode?
        guard SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess, let staticCode else {
            throw LocalRuntimeServiceError.untrusted
        }
        try verifyHardened(staticCode)
    }

    private static func verifyHardened(_ code: SecStaticCode) throws {
        var information: CFDictionary?
        guard SecCodeCopySigningInformation(code, SecCSFlags(rawValue: kSecCSSigningInformation), &information) == errSecSuccess,
              let flags = (information as? [String: Any])?[kSecCodeInfoFlags as String] as? NSNumber,
              flags.uint32Value & 0x10000 != 0 else { throw LocalRuntimeServiceError.untrusted }
    }
}

private final class NativeRuntimeTrustDelegate: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    private let fingerprint: String
    init(fingerprint: String) { self.fingerprint = fingerprint.lowercased() }

    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        validate(challenge, completionHandler: completionHandler)
    }
    func urlSession(_ session: URLSession, task: URLSessionTask, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        validate(challenge, completionHandler: completionHandler)
    }
    private func validate(_ challenge: URLAuthenticationChallenge,
                          completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              challenge.protectionSpace.host == "127.0.0.1",
              let trust = challenge.protectionSpace.serverTrust,
              let certificates = SecTrustCopyCertificateChain(trust) as? [SecCertificate], let certificate = certificates.first,
              LocalRuntimeService.isSecret(fingerprint),
              SHA256.hash(data: SecCertificateCopyData(certificate) as Data).map({ String(format: "%02x", $0) }).joined() == fingerprint
        else { completionHandler(.cancelAuthenticationChallenge, nil); return }
        SecTrustSetNetworkFetchAllowed(trust, false)
        SecTrustSetPolicies(trust, SecPolicyCreateSSL(true, "127.0.0.1" as CFString))
        SecTrustSetAnchorCertificates(trust, [certificate] as CFArray)
        SecTrustSetAnchorCertificatesOnly(trust, true)
        guard SecTrustEvaluateWithError(trust, nil) else {
            completionHandler(.cancelAuthenticationChallenge, nil); return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}
