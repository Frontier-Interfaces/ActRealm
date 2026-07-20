import Combine
import Darwin
import Foundation

/// Locates the `actrealm` Rust backend (bundled helper first, then a dev
/// checkout), supervises it as a child process, and extracts the one-time
/// bootstrap URL/token it prints on startup so a `RuntimeClient` can hand
/// off into an authenticated session.
@MainActor
public final class RuntimeSupervisor: ObservableObject {
    public enum State: Equatable, Sendable {
        case idle
        case buildingBackend
        case launching
        case restarting
        case running
        case stopped
        case failed(String)
    }

    public struct Diagnostics: Equatable, Sendable {
        public let checkedAt: Date
        public let managedPID: Int32?
        public let lockOwnerPID: Int32?
        public let lockOwnerPath: String?
        public let lockOwnerIsAlive: Bool
        public let socketExists: Bool
        public let helperPath: String?
        public let endpoint: String?
        public let stdoutTail: String
        public let stderrTail: String
        public let launchAgentWarning: String?

        public static let empty = Diagnostics(
            checkedAt: .distantPast,
            managedPID: nil,
            lockOwnerPID: nil,
            lockOwnerPath: nil,
            lockOwnerIsAlive: false,
            socketExists: false,
            helperPath: nil,
            endpoint: nil,
            stdoutTail: "",
            stderrTail: "",
            launchAgentWarning: nil
        )
    }

    @Published public private(set) var state: State = .idle
    @Published public private(set) var diagnostics: Diagnostics = .empty

    private let repoPath: URL?
    private var process: Process?
    private var stdoutBuffer = Data()
    private var stdoutTail = ""
    private var stderrTail = ""
    private var endpoint: URL?
    private var onBootstrap: ((URL, String) -> Void)?

    /// - Parameter repoPath: dev checkout of the Rust workspace used as a
    ///   fallback when the app bundle does not contain a helper binary.
    public init(repoPath: URL? = nil) {
        self.repoPath = repoPath
    }

    /// The packaged helper inside the .app bundle, if present.
    public nonisolated static func bundledHelper() -> URL? {
        let bundle = Bundle.main
        if let helper = bundle.url(forAuxiliaryExecutable: "actrealm"),
           FileManager.default.isExecutableFile(atPath: helper.path) {
            return helper
        }
        let helpersDir = bundle.bundleURL
            .appendingPathComponent("Contents/Helpers/actrealm")
        if FileManager.default.isExecutableFile(atPath: helpersDir.path) {
            return helpersDir
        }
        return nil
    }

    /// Ensures a backend binary exists (bundled helper, or a cargo release
    /// build of the dev checkout), launches `actrealm serve`, and invokes
    /// `onBootstrap` exactly once with the base URL and one-time token
    /// parsed from its stdout.
    public func start(onBootstrap: @escaping (URL, String) -> Void) async {
        self.onBootstrap = onBootstrap
        refreshDiagnostics()

        if let bundled = Self.bundledHelper() {
            state = .launching
            launch(binary: bundled, workingDirectory: nil)
            return
        }

        guard let repoPath else {
            state = .failed("app 内没有打包 actrealm Helper，也未配置开发仓库路径")
            return
        }
        let binary = repoPath.appendingPathComponent("target/release/actrealm")
        if !FileManager.default.fileExists(atPath: binary.path) {
            state = .buildingBackend
            let succeeded = await Self.buildRelease(repoPath: repoPath)
            guard succeeded else {
                state = .failed("cargo build --release -p actrealm failed")
                return
            }
        }
        state = .launching
        launch(binary: binary, workingDirectory: repoPath)
    }

    public func stop() {
        onBootstrap = nil
        guard let process, process.isRunning else { return }
        process.terminate()
        state = .stopped
        refreshDiagnostics()
    }

    /// Restarts the app-managed Runtime. If an earlier copy of this app left
    /// its bundled helper holding `runtime.lock`, the lock owner is stopped
    /// only after its executable path is verified as a known actrealm path.
    @discardableResult
    public func restart(onBootstrap: @escaping (URL, String) -> Void) async -> String? {
        state = .restarting
        self.onBootstrap = nil
        refreshDiagnostics()

        if await bootOutOutdatedLaunchAgent() {
            stdoutTail = String((stdoutTail + "已停止参数过期的 com.frontier.actrealm.runtime LaunchAgent\n").suffix(4000))
        }

        if let process, process.isRunning {
            await terminate(processID: process.processIdentifier)
        }
        process = nil

        if let owner = Self.lockOwnerPID(), Self.isProcessAlive(owner) {
            let ownerPath = Self.processPath(owner)
            guard isExpectedRuntimePath(ownerPath) else {
                let path = ownerPath ?? "未知路径"
                let message = "runtime.lock 由未识别进程 PID \(owner) 持有（\(path)），为避免误杀未自动停止"
                state = .failed(message)
                refreshDiagnostics()
                return message
            }
            await terminate(processID: owner)
            if Self.isProcessAlive(owner) {
                let message = "无法停止旧 Runtime（PID \(owner)）"
                state = .failed(message)
                refreshDiagnostics()
                return message
            }
        }

        stdoutBuffer.removeAll(keepingCapacity: true)
        stdoutTail = ""
        stderrTail = ""
        endpoint = nil
        await start(onBootstrap: onBootstrap)
        return nil
    }

    private nonisolated static func buildRelease(repoPath: URL) async -> Bool {
        await withCheckedContinuation { continuation in
            let proc = Process()
            proc.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            proc.arguments = ["cargo", "build", "--release", "-p", "actrealm"]
            proc.currentDirectoryURL = repoPath
            proc.terminationHandler = { finished in
                continuation.resume(returning: finished.terminationStatus == 0)
            }
            do {
                try proc.run()
            } catch {
                continuation.resume(returning: false)
            }
        }
    }

    private func launch(binary: URL, workingDirectory: URL?) {
        let proc = Process()
        proc.executableURL = binary
        proc.arguments = ["serve"]
        if let workingDirectory {
            proc.currentDirectoryURL = workingDirectory
        }
        let stdout = Pipe()
        let stderr = Pipe()
        proc.standardOutput = stdout
        proc.standardError = stderr

        proc.terminationHandler = { [weak self] finished in
            let status = finished.terminationStatus
            Task { @MainActor in
                guard let self, self.process === proc else { return }
                if status == 0 {
                    self.state = .stopped
                } else {
                    self.state = .failed(Self.failureMessage(status: status, stderr: self.stderrTail))
                }
                self.refreshDiagnostics()
            }
        }
        stdout.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty else { return }
            Task { @MainActor in self?.ingest(data) }
        }
        stderr.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            guard !data.isEmpty, let text = String(data: data, encoding: .utf8) else { return }
            Task { @MainActor in
                guard let self else { return }
                self.stderrTail = String((self.stderrTail + text).suffix(4000))
                self.refreshDiagnostics()
            }
        }

        do {
            try proc.run()
            process = proc
            refreshDiagnostics()
        } catch {
            state = .failed("failed to launch actrealm: \(error.localizedDescription)")
            refreshDiagnostics()
        }
    }

    private nonisolated static func failureMessage(status: Int32, stderr: String) -> String {
        if stderr.contains("failed to acquire") && stderr.contains(".lock") {
            return "另一个 actrealm 实例已在运行（可先退出终端里的 serve）"
        }
        let tail = stderr
            .split(separator: "\n", omittingEmptySubsequences: true)
            .suffix(2)
            .joined(separator: " · ")
        return tail.isEmpty ? "actrealm 退出（状态码 \(status)）" : tail
    }

    private func ingest(_ data: Data) {
        stdoutBuffer.append(data)
        while let newline = stdoutBuffer.firstIndex(of: 0x0A) {
            let lineData = stdoutBuffer[stdoutBuffer.startIndex..<newline]
            stdoutBuffer.removeSubrange(stdoutBuffer.startIndex...newline)
            if let line = String(data: lineData, encoding: .utf8) {
                stdoutTail = String((stdoutTail + line + "\n").suffix(4000))
                consume(line: line)
            }
        }
        refreshDiagnostics()
    }

    private func consume(line: String) {
        guard onBootstrap != nil, let parsed = Self.parseBootstrapLine(line) else { return }
        let callback = onBootstrap
        onBootstrap = nil
        state = .running
        endpoint = parsed.baseURL
        refreshDiagnostics()
        callback?(parsed.baseURL, parsed.token)
    }

    public func refreshDiagnostics() {
        let owner = Self.lockOwnerPID()
        let ownerAlive = owner.map(Self.isProcessAlive) ?? false
        diagnostics = Diagnostics(
            checkedAt: Date(),
            managedPID: process.flatMap { $0.isRunning ? $0.processIdentifier : nil },
            lockOwnerPID: owner,
            lockOwnerPath: owner.flatMap(Self.processPath),
            lockOwnerIsAlive: ownerAlive,
            socketExists: FileManager.default.fileExists(atPath: Self.bridgeSocketURL.path),
            helperPath: resolvedHelper()?.path,
            endpoint: endpoint?.absoluteString,
            stdoutTail: stdoutTail,
            stderrTail: stderrTail,
            launchAgentWarning: Self.launchAgentWarning()
        )
    }

    public nonisolated static func parseLockOwnerPID(_ text: String) -> Int32? {
        Int32(text.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private func resolvedHelper() -> URL? {
        if let bundled = Self.bundledHelper() { return bundled }
        guard let repoPath else { return nil }
        return repoPath.appendingPathComponent("target/release/actrealm")
    }

    private func isExpectedRuntimePath(_ path: String?) -> Bool {
        guard let path else { return false }
        var expected = [
            Self.installedHelperURL.path,
            Self.bundledHelper()?.path,
            repoPath?.appendingPathComponent("target/release/actrealm").path,
        ].compactMap { $0 }
        if let process, process.executableURL?.path != nil {
            expected.append(process.executableURL!.path)
        }
        return expected.contains(path)
    }

    private func terminate(processID: Int32) async {
        guard Self.isProcessAlive(processID) else { return }
        kill(processID, SIGTERM)
        let deadline = Date().addingTimeInterval(3)
        while Self.isProcessAlive(processID), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(100))
        }
        if Self.isProcessAlive(processID) {
            kill(processID, SIGKILL)
        }
        let killDeadline = Date().addingTimeInterval(1)
        while Self.isProcessAlive(processID), Date() < killDeadline {
            try? await Task.sleep(for: .milliseconds(100))
        }
        refreshDiagnostics()
    }

    private func bootOutOutdatedLaunchAgent() async -> Bool {
        guard Self.launchAgentWarning() != nil else { return false }
        return await withCheckedContinuation { continuation in
            let proc = Process()
            proc.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            proc.arguments = ["bootout", "gui/\(getuid())/com.frontier.actrealm.runtime"]
            proc.standardOutput = Pipe()
            proc.standardError = Pipe()
            proc.terminationHandler = { finished in
                continuation.resume(returning: finished.terminationStatus == 0)
            }
            do {
                try proc.run()
            } catch {
                continuation.resume(returning: false)
            }
        }
    }

    private nonisolated static var actRealmHome: URL {
        FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".actrealm")
    }

    private nonisolated static var runtimeLockURL: URL {
        actRealmHome.appendingPathComponent("run/runtime.lock")
    }

    private nonisolated static var bridgeSocketURL: URL {
        actRealmHome.appendingPathComponent("run/bridge.sock")
    }

    private nonisolated static var installedHelperURL: URL {
        actRealmHome.appendingPathComponent("bin/actrealm")
    }

    private nonisolated static func lockOwnerPID() -> Int32? {
        guard let text = try? String(contentsOf: runtimeLockURL, encoding: .utf8) else { return nil }
        return parseLockOwnerPID(text)
    }

    private nonisolated static func isProcessAlive(_ pid: Int32) -> Bool {
        guard pid > 0 else { return false }
        if kill(pid, 0) == 0 { return true }
        return errno == EPERM
    }

    private nonisolated static func processPath(_ pid: Int32) -> String? {
        var buffer = [CChar](repeating: 0, count: 4096)
        let count = proc_pidpath(pid, &buffer, UInt32(buffer.count))
        guard count > 0 else { return nil }
        let bytes = buffer.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }
        return String(decoding: bytes, as: UTF8.self)
    }

    private nonisolated static func launchAgentWarning() -> String? {
        let plistURL = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/com.frontier.actrealm.runtime.plist")
        guard let data = try? Data(contentsOf: plistURL),
              let plist = try? PropertyListSerialization.propertyList(from: data, format: nil),
              let dictionary = plist as? [String: Any],
              let arguments = dictionary["ProgramArguments"] as? [String]
        else { return nil }
        if arguments.contains("--port") {
            return "发现旧 LaunchAgent：serve --port 已不受当前 Runtime 支持，会反复退出"
        }
        return nil
    }

    /// Parses `ActRealm control panel: http://127.0.0.1:<port>/#bootstrap=<token>`.
    public nonisolated static func parseBootstrapLine(_ line: String) -> (baseURL: URL, token: String)? {
        guard let prefixRange = line.range(of: "ActRealm control panel: ") else { return nil }
        let rest = line[prefixRange.upperBound...]
        guard let markerRange = rest.range(of: "/#bootstrap=") else { return nil }
        let baseURLString = String(rest[rest.startIndex..<markerRange.lowerBound])
        let token = String(rest[markerRange.upperBound...])
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let baseURL = URL(string: baseURLString), !token.isEmpty else { return nil }
        return (baseURL, token)
    }
}
