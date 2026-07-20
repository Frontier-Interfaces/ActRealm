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
}
