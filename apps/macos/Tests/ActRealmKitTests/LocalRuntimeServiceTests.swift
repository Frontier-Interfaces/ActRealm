import Foundation
import Testing
@testable import ActRealmKit

@Suite struct LocalRuntimeServiceTests {
    private func credentials(endpoint: String, version: Int = 7, instance: String = "a7e2d3c4-7b9a-4d01-8012-123456789abc") -> LocalRuntimeCredentials {
        LocalRuntimeCredentials(schemaVersion: 1, protocolVersion: version, endpoint: endpoint,
            instanceId: instance, sessionToken: nil, csrfToken: nil, token: nil,
            companionId: nil, scopes: nil, discoveryPath: nil, certificateSha256: String(repeating: "f", count: 64))
    }

    @Test func nativeConnectionRejectsRemoteOrCredentialBearingEndpoints() {
        #expect(credentials(endpoint: "https://127.0.0.1:43121").isSupported)
        for endpoint in ["http://example.com:43121", "https://127.0.0.1", "https://127.0.0.1:0",
                         "https://user:secret@127.0.0.1:43121", "https://127.0.0.1:43121/?token=secret",
                         "https://127.0.0.1:43121/#bootstrap=secret", "https://127.0.0.1:43121/other"] {
            #expect(!credentials(endpoint: endpoint).isSupported)
        }
        #expect(!credentials(endpoint: "https://127.0.0.1:43121", version: 6).isSupported)
        #expect(!credentials(endpoint: "https://127.0.0.1:43121", instance: "unverified").isSupported)
    }

    @Test func invalidLegacyCredentialsRequireExplicitEnableInsteadOfBeingSilentlyDiscarded() {
        #expect(LocalRuntimeService.isSecret(String(repeating: "a", count: 64)))
        #expect(!LocalRuntimeService.isSecret(String(repeating: "x", count: 64)))
        #expect(!LocalRuntimeService.isSecret(String(repeating: "a", count: 63) + "\n"))
        do {
            _ = try LocalRuntimeService.enroll(socket: URL(fileURLWithPath: "/nonexistent/socket"),
                team: "TEST", previousToken: "corrupt", enableAccess: false)
            Issue.record("invalid legacy credentials were silently accepted")
        } catch LocalRuntimeServiceError.accessRevoked { }
        catch { Issue.record("unexpected error: \(error)") }
    }
}
