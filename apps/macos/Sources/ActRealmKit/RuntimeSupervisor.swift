import Combine
import Darwin
import Foundation

public struct RuntimeDoctorReport: Codable, Equatable, Sendable {
    public struct Check: Codable, Equatable, Identifiable, Sendable {
        public let id: String
        public let status: String
        public let summary: String
        public let detail: String
        public let repairability: String
        public let action: String?
    }

    public let schemaVersion: UInt16
    public let generatedAtMs: UInt64
    public let overall: String
    public let checks: [Check]

    public func check(_ id: String) -> Check? {
        checks.first { $0.id == id }
    }

    public static let preview = RuntimeDoctorReport(
        schemaVersion: 1,
        generatedAtMs: 1_800_000_000_000,
        overall: "pass",
        checks: [
            Check(
                id: "claude.cli",
                status: "pass",
                summary: "claude CLI is available",
                detail: "/usr/local/bin/claude · 2.1.226 (Claude Code)",
                repairability: "not_applicable",
                action: nil
            ),
            Check(
                id: "codex.cli",
                status: "pass",
                summary: "codex CLI is available",
                detail: "/usr/local/bin/codex · codex-cli 0.144.6",
                repairability: "not_applicable",
                action: nil
            )
        ]
    )
}

/// Connects to the independent per-user Runtime service. Closing this client
/// never stops the shared service or another application's connection.
@MainActor
public final class RuntimeSupervisor: ObservableObject {
    public struct ProcessResult: Equatable, Sendable {
        public let status: Int32
        public let stdout: String
        public let stderr: String
        public let executedOnMainThread: Bool
    }

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
    private var stdoutTail = ""
    private var stderrTail = ""
    private var endpoint: URL?
    private var connectionHandler: ((LocalRuntimeCredentials) -> Void)?
    private var connectionMonitor: Task<Void, Never>?
    private var localCredentials: LocalRuntimeCredentials?
    private var connectionGeneration = 0
    private var desiredRunning = false
    private var consecutiveFailures = 0

    /// - Parameter repoPath: dev checkout of the Rust workspace used as a
    ///   fallback when the app bundle does not contain a helper binary.
    public init(repoPath: URL? = nil) {
        self.repoPath = repoPath
    }

    /// The packaged helper inside the .app bundle, if present.
    public nonisolated static func bundledHelper() -> URL? {
        let bundle = Bundle.main
        return resolveBundledHelper(
            bundleURL: bundle.bundleURL,
            mainExecutableURL: bundle.executableURL,
            auxiliaryExecutableURL: bundle.url(forAuxiliaryExecutable: "actrealm")
        )
    }

    nonisolated static func resolveBundledHelper(
        bundleURL: URL,
        mainExecutableURL: URL?,
        auxiliaryExecutableURL: URL?,
        fileManager: FileManager = .default
    ) -> URL? {
        let packagedHelper = bundleURL.appendingPathComponent("Contents/Helpers/actrealm")
        if fileManager.isExecutableFile(atPath: packagedHelper.path) {
            return packagedHelper
        }

        guard let auxiliaryExecutableURL,
              fileManager.isExecutableFile(atPath: auxiliaryExecutableURL.path),
              !refersToSameFile(auxiliaryExecutableURL, mainExecutableURL, fileManager: fileManager)
        else { return nil }
        return auxiliaryExecutableURL
    }

    private nonisolated static func refersToSameFile(
        _ candidate: URL,
        _ mainExecutable: URL?,
        fileManager: FileManager
    ) -> Bool {
        guard let mainExecutable else { return false }
        if candidate.standardizedFileURL == mainExecutable.standardizedFileURL {
            return true
        }

        guard let candidateAttributes = try? fileManager.attributesOfItem(atPath: candidate.path),
              let mainAttributes = try? fileManager.attributesOfItem(atPath: mainExecutable.path),
              let candidateDevice = candidateAttributes[.systemNumber] as? NSNumber,
              let mainDevice = mainAttributes[.systemNumber] as? NSNumber,
              let candidateFile = candidateAttributes[.systemFileNumber] as? NSNumber,
              let mainFile = mainAttributes[.systemFileNumber] as? NSNumber
        else { return false }

        return candidateDevice == mainDevice && candidateFile == mainFile
    }

    public func start(onConnect: @escaping (LocalRuntimeCredentials) -> Void) async {
        desiredRunning = true
        connectionHandler = onConnect
        connectionGeneration += 1
        connectionMonitor?.cancel()
        connectionMonitor = nil
        await launchConfiguredRuntime()
        startConnectionMonitor()
    }

    private func launchConfiguredRuntime() async {
        guard desiredRunning else { return }
        let generation = connectionGeneration
        state = .launching
        do {
            let helper = try await serviceHelper()
            let credentials = try await Task.detached(priority: .utility) {
                try LocalRuntimeService.connect(helper: helper)
            }.value
            guard desiredRunning, generation == connectionGeneration else { return }
            localCredentials = credentials
            endpoint = credentials.baseURL
            state = .running
            consecutiveFailures = 0
            connectionHandler?(credentials)
            refreshDiagnostics()
        } catch {
            guard desiredRunning, generation == connectionGeneration else { return }
            localCredentials = nil
            state = .failed(error.localizedDescription)
            stderrTail = (error as? LocalRuntimeServiceError)?.diagnosticDescription ?? error.localizedDescription
            consecutiveFailures += 1
            refreshDiagnostics()
        }
    }

    private func serviceHelper() async throws -> URL {
        if let helper = Self.bundledHelper() { return helper }
        guard let repoPath else { throw LocalRuntimeServiceError.unavailable }
        let binary = repoPath.appendingPathComponent("target/release/actrealm")
        if !FileManager.default.isExecutableFile(atPath: binary.path) {
            state = .buildingBackend
            guard await Self.buildRelease(repoPath: repoPath) else { throw LocalRuntimeServiceError.unavailable }
        }
        return binary
    }

    private func startConnectionMonitor() {
        connectionMonitor?.cancel()
        connectionMonitor = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(2))
                guard let self, self.desiredRunning, !Task.isCancelled else { return }
                if let credentials = self.localCredentials, await LocalRuntimeService.isCurrent(credentials) { continue }
                guard self.consecutiveFailures < 5 else { return }
                await self.launchConfiguredRuntime()
            }
        }
    }

    public func stop() {
        desiredRunning = false
        connectionGeneration += 1
        connectionMonitor?.cancel()
        connectionMonitor = nil
        connectionHandler = nil
        localCredentials = nil
        state = .stopped
        // launchd owns Runtime. This operation only disconnects the app.
        refreshDiagnostics()
    }

    @discardableResult
    public func restart(onConnect: @escaping (LocalRuntimeCredentials) -> Void) async -> String? {
        desiredRunning = true
        connectionHandler = onConnect
        connectionGeneration += 1
        let generation = connectionGeneration
        connectionMonitor?.cancel()
        connectionMonitor = nil
        state = .restarting
        do {
            let helper = try await serviceHelper()
            let credentials = try await Task.detached(priority: .utility) {
                try LocalRuntimeService.connect(helper: helper, restart: true)
            }.value
            guard desiredRunning, generation == connectionGeneration else { return nil }
            localCredentials = credentials
            endpoint = credentials.baseURL
            state = .running
            consecutiveFailures = 0
            connectionHandler?(credentials)
            refreshDiagnostics()
            startConnectionMonitor()
            return nil
        } catch {
            state = .failed(error.localizedDescription)
            stderrTail = (error as? LocalRuntimeServiceError)?.diagnosticDescription ?? error.localizedDescription
            refreshDiagnostics()
            return error.localizedDescription
        }
    }

    private nonisolated static func buildRelease(repoPath: URL) async -> Bool {
        let result = await runProcess(
            executable: "/usr/bin/env",
            arguments: ["cargo", "build", "--release", "-p", "actrealm"],
            currentDirectory: repoPath
        )
        return result.status == 0
    }

    /// Bootstrap credentials are one-time secrets. Diagnostics may retain the
    /// endpoint for support, but must never persist or render the token.
    public nonisolated static func redactedDiagnosticText(_ text: String) -> String {
        guard let expression = try? NSRegularExpression(
            pattern: #"(?i)([#?&]bootstrap=)[^\s"'<>]+"#
        ) else { return text }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return expression.stringByReplacingMatches(
            in: text,
            range: range,
            withTemplate: "$1<redacted>"
        )
    }

    public nonisolated static func automaticRestartDelay(attempt: Int) -> TimeInterval? {
        let delays: [TimeInterval] = [0.5, 1, 2, 4, 8]
        guard delays.indices.contains(attempt) else { return nil }
        return delays[attempt]
    }

    public func refreshDiagnostics() {
        let owner = Self.lockOwnerPID()
        let ownerAlive = owner.map(Self.isProcessAlive) ?? false
        diagnostics = Diagnostics(
            checkedAt: Date(),
            managedPID: nil,
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

    public func doctorReport() async -> RuntimeDoctorReport? {
        guard let helper = resolvedHelper(),
              FileManager.default.isExecutableFile(atPath: helper.path)
        else { return nil }
        let result = await Self.runProcess(
            executable: helper.path,
            arguments: ["doctor", "--json"],
            retainedBytes: 128 * 1024
        )
        guard result.status == 0 || !result.stdout.isEmpty else { return nil }
        return Self.decodeDoctorReport(Data(result.stdout.utf8))
    }

    public nonisolated static func decodeDoctorReport(
        _ data: Data
    ) -> RuntimeDoctorReport? {
        try? JSONDecoder().decode(RuntimeDoctorReport.self, from: data)
    }

    public nonisolated static func parseLockOwnerPID(_ text: String) -> Int32? {
        Int32(text.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private func resolvedHelper() -> URL? {
        if let bundled = Self.bundledHelper() { return bundled }
        guard let repoPath else { return nil }
        return repoPath.appendingPathComponent("target/release/actrealm")
    }

    nonisolated static func isCompatibleAppHelperIdentity(
        currentBundleID: String?,
        candidateBundleID: String?,
        currentTeamID: String?,
        candidateTeamID: String?,
        candidateOwnerID: UInt32?,
        currentUserID: UInt32
    ) -> Bool {
        guard let currentBundleID, candidateBundleID == currentBundleID else { return false }
        if let currentTeamID, !currentTeamID.isEmpty {
            return candidateTeamID == currentTeamID
        }
        return candidateTeamID == nil && candidateOwnerID == currentUserID
    }

    /// Runs a short-lived child away from MainActor and drains stdout/stderr
    /// concurrently so a full pipe cannot deadlock the app. Only the bounded
    /// tail is retained, and bootstrap credentials are redacted by default.
    public nonisolated static func runProcess(
        executable: String,
        arguments: [String],
        currentDirectory: URL? = nil,
        retainedBytes: Int = 64 * 1024,
        redactDiagnostics: Bool = true
    ) async -> ProcessResult {
        await Task.detached(priority: .utility) {
            let process = Process()
            let stdoutPipe = Pipe()
            let stderrPipe = Pipe()
            process.executableURL = URL(fileURLWithPath: executable)
            process.arguments = arguments
            process.currentDirectoryURL = currentDirectory
            process.standardOutput = stdoutPipe
            process.standardError = stderrPipe
            let mainThread = pthread_main_np() != 0
            do {
                try process.run()
            } catch {
                return ProcessResult(
                    status: -1,
                    stdout: "",
                    stderr: error.localizedDescription,
                    executedOnMainThread: mainThread
                )
            }

            async let stdoutData = drain(stdoutPipe.fileHandleForReading, retaining: retainedBytes)
            async let stderrData = drain(stderrPipe.fileHandleForReading, retaining: retainedBytes)
            process.waitUntilExit()
            let stdout = String(decoding: await stdoutData, as: UTF8.self)
            let stderr = String(decoding: await stderrData, as: UTF8.self)
            return ProcessResult(
                status: process.terminationStatus,
                stdout: redactDiagnostics ? redactedDiagnosticText(stdout) : stdout,
                stderr: redactDiagnostics ? redactedDiagnosticText(stderr) : stderr,
                executedOnMainThread: mainThread
            )
        }.value
    }

    private nonisolated static func drain(_ handle: FileHandle, retaining limit: Int) async -> Data {
        var retained = Data()
        while !Task.isCancelled {
            do {
                guard let chunk = try handle.read(upToCount: 16 * 1024), !chunk.isEmpty else { break }
                retained.append(chunk)
                if retained.count > max(0, limit) {
                    retained.removeFirst(retained.count - max(0, limit))
                }
            } catch {
                break
            }
        }
        return retained
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
