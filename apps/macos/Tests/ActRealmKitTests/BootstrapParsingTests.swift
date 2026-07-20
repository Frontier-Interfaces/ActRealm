import Foundation
import Testing
@testable import ActRealmKit

struct BootstrapParsingTests {
    @Test func parsesRealServeOutputLine() throws {
        let line = "ActRealm control panel: http://127.0.0.1:54321/#bootstrap=0199b3b2-1a2b-7c3d-8e4f-abcdef123456"
        let parsed = try #require(RuntimeSupervisor.parseBootstrapLine(line))
        #expect(parsed.baseURL.absoluteString == "http://127.0.0.1:54321")
        #expect(parsed.token == "0199b3b2-1a2b-7c3d-8e4f-abcdef123456")
    }

    @Test func trimsTrailingNewlineFromToken() throws {
        let line = "ActRealm control panel: http://127.0.0.1:8080/#bootstrap=one-time-token\n"
        let parsed = try #require(RuntimeSupervisor.parseBootstrapLine(line))
        #expect(parsed.token == "one-time-token")
    }

    @Test func ignoresUnrelatedLines() {
        #expect(RuntimeSupervisor.parseBootstrapLine("actrealm runtime listening on /tmp/actrealm.sock") == nil)
        #expect(RuntimeSupervisor.parseBootstrapLine("") == nil)
    }

    @Test func parsesRuntimeLockOwnerPID() {
        #expect(RuntimeSupervisor.parseLockOwnerPID("27489\n") == 27489)
        #expect(RuntimeSupervisor.parseLockOwnerPID("  42  ") == 42)
        #expect(RuntimeSupervisor.parseLockOwnerPID("not-a-pid") == nil)
    }

    @Test func packagedHelperWinsOverAuxiliaryExecutableLookup() throws {
        let fixture = try HelperBundleFixture()
        defer { fixture.remove() }

        let resolved = RuntimeSupervisor.resolveBundledHelper(
            bundleURL: fixture.bundleURL,
            mainExecutableURL: fixture.mainExecutableURL,
            auxiliaryExecutableURL: fixture.mainExecutableURL
        )

        #expect(resolved == fixture.helperURL)
    }

    @Test func mainExecutableIsNeverAcceptedAsRuntimeHelper() throws {
        let fixture = try HelperBundleFixture(includeHelper: false)
        defer { fixture.remove() }

        let resolved = RuntimeSupervisor.resolveBundledHelper(
            bundleURL: fixture.bundleURL,
            mainExecutableURL: fixture.mainExecutableURL,
            auxiliaryExecutableURL: fixture.mainExecutableURL
        )

        #expect(resolved == nil)
    }
}

private struct HelperBundleFixture {
    let rootURL: URL
    let bundleURL: URL
    let mainExecutableURL: URL
    let helperURL: URL

    init(includeHelper: Bool = true) throws {
        rootURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("actrealm-helper-test-\(UUID().uuidString)")
        bundleURL = rootURL.appendingPathComponent("ActRealm.app")
        mainExecutableURL = bundleURL.appendingPathComponent("Contents/MacOS/ActRealm")
        helperURL = bundleURL.appendingPathComponent("Contents/Helpers/actrealm")

        try FileManager.default.createDirectory(
            at: mainExecutableURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try Data("app".utf8).write(to: mainExecutableURL)
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o755],
            ofItemAtPath: mainExecutableURL.path
        )

        if includeHelper {
            try FileManager.default.createDirectory(
                at: helperURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try Data("helper".utf8).write(to: helperURL)
            try FileManager.default.setAttributes(
                [.posixPermissions: 0o755],
                ofItemAtPath: helperURL.path
            )
        }
    }

    func remove() {
        try? FileManager.default.removeItem(at: rootURL)
    }
}
