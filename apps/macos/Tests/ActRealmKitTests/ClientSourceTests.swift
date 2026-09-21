import Foundation
import Testing
@testable import ActRealmKit

struct ClientSourceTests {
    @Test func missingSourceNeverBecomesAProviderCLIOrAProjectName() throws {
        for provider in ["codex", "claude", "kimi", "grok", "gemini", "custom"] {
            let base: [String: Any] = ["id": "session", "provider": provider,
                "providerSessionId": "native-session", "execState": "thinking",
                "project": "my-project", "lastEventAt": 1000]
            for environment in [nil, "", "  \n "] as [String?] {
                var value = base
                if let environment { value["environment"] = environment }
                let session = try JSONDecoder().decode(SessionRecord.self,
                    from: JSONSerialization.data(withJSONObject: value))
                #expect(session.clientSourceLabelKey == "来源未识别")
            }
        }
    }

    @Test func observedClientStaysIndependentOfTheProviderAndManagedConnection() throws {
        let value: [String: Any] = ["id": "session", "provider": "codex",
            "providerSessionId": "native-session", "execState": "thinking",
            "lastEventAt": 1000, "environment": "Cursor", "controlCapability": "external_hook",
            "managedConnectionState": "owned_elsewhere"]
        let session = try JSONDecoder().decode(SessionRecord.self,
            from: JSONSerialization.data(withJSONObject: value))
        #expect(session.clientSourceLabelKey == "Cursor")
        #expect(session.controlCapability == "external_hook")
    }
}
