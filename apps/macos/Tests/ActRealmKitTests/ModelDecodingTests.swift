import Foundation
import Testing
@testable import ActRealmKit

struct ModelDecodingTests {
    @Test func decodesSnapshotShapeFromServer() throws {
        let json = """
        {
          "sessions": [
            {
              "id": "sess-1",
              "provider": "claude",
              "providerSessionId": "abc123",
              "project": "actrealm",
              "title": "Refactor server auth",
              "providerTitle": "Fix macOS parity",
              "providerTitleSource": "session_meta",
              "model": "claude-sonnet-5",
              "execState": "tool_running",
              "approvalOwner": null,
              "activity": "Editing files",
              "activitySince": 1737000000000,
              "planDone": 2,
              "planTotal": 5,
              "inputTokens": 48000,
              "outputTokens": 1300,
              "turnStartedAt": 1737000000000,
              "tokenTotal": 49300,
              "contextWindowTokens": 258400,
              "lastTurnTokens": 2300,
              "contextUsedTokens": 49300,
              "contextUsedPercent": 19,
              "estimatedCostUsdMicros": 123456,
              "currentTool": "Edit",
              "activeSubagents": 2,
              "environment": "Terminal · zsh",
              "jumpCapability": "terminal",
              "jumpLabel": "返回 Terminal",
              "controlCapability": "external_hook",
              "recoveryState": "observing",
              "canManage": false,
              "usageCapturedAt": 1737000090000,
              "lastEventAt": 1737000100000
            }
          ],
          "attention": [
            {
              "id": "att-1",
              "sessionId": "sess-1",
              "provider": "claude",
              "project": "actrealm",
              "requestId": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
              "kind": "approval",
              "title": "Run rm -rf build/",
              "detail": "Requested by session sess-1",
              "state": "open",
              "risk": "high",
              "riskNotes": ["destructive"],
              "commandPreview": "rm -rf build/",
              "expiresAt": null,
              "createdAt": 1737000100000,
              "resolution": null
            },
            {
              "id": "att-question",
              "sessionId": "sess-1",
              "provider": "claude",
              "project": "actrealm",
              "requestId": "00000000-0000-0000-0000-000000000002",
              "kind": "question",
              "title": "选择交付方式",
              "detail": null,
              "state": "open",
              "risk": "low",
              "riskNotes": [],
              "commandPreview": null,
              "expiresAt": 1737003700000,
              "createdAt": 1737000100000,
              "resolution": null,
              "interaction": {
                "requestId": "00000000-0000-0000-0000-000000000002",
                "kind": "claude_question",
                "provider": "claude",
                "title": "选择交付方式",
                "message": "请选择一项",
                "expiresAt": 1737003700000,
                "supportsNative": true,
                "questions": [{
                  "id": "delivery",
                  "label": "交付",
                  "prompt": "如何交付？",
                  "inputType": "choice",
                  "multiSelect": false,
                  "isSecret": false,
                  "required": true,
                  "allowsOther": true,
                  "options": [{"label":"PR","description":"创建草稿 PR"}]
                }]
              }
            }
          ],
          "commands": [],
          "quota": [
            {
              "provider": "claude",
              "window": "5h",
              "status": "available",
              "usedPct": 42.5,
              "remainingPct": 57.5,
              "resetsAt": 1737003600,
              "source": "statusline",
              "windowMinutes": 300,
              "limitId": "five_hour",
              "limitName": "5 小时",
              "planType": "Max",
              "capturedAt": 1737000000000
            },
            {
              "provider": "codex",
              "window": "week",
              "status": "unavailable",
              "source": "unsupported_version",
              "reason": "unsupported desktop version"
            }
          ],
          "stats": {
            "eventCount": 12,
            "metrics": {
              "activeDays": 3,
              "approvalRequests": 4,
              "widgetApprovals": 2,
              "widgetDenials": 1,
              "passThroughManual": 0,
              "passThroughTimeout": 1,
              "decisionResponseMsTotal": 5000,
              "decisionResponseCount": 3,
              "bannersShown": 6,
              "sessionsObserved": 2,
              "appOpened": 5,
              "todayWidgetDecisions": 2
            }
          },
          "capabilities": {
            "codexConnector": {
              "enabled": true,
              "status": "connected",
              "managedThreads": 1,
              "error": null
            }
          }
        }
        """

        let snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8))

        #expect(snapshot.sessions.count == 1)
        #expect(snapshot.sessions[0].providerSessionId == "abc123")
        #expect(snapshot.sessions[0].providerTitle == "Fix macOS parity")
        #expect(snapshot.sessions[0].planDone == 2)
        #expect(snapshot.sessions[0].totalTokens == 49_300)
        #expect(snapshot.sessions[0].contextWindowTokens == 258_400)
        #expect(snapshot.sessions[0].contextUsedPercent == 19)
        #expect(snapshot.sessions[0].estimatedCostUsdMicros == 123_456)
        #expect(snapshot.sessions[0].controlCapability == "external_hook")

        #expect(snapshot.attention.count == 2)
        #expect(snapshot.attention[0].kind == "approval")
        #expect(snapshot.attention[0].requestId?.uuidString.lowercased() == "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d")
        #expect(snapshot.attention[0].riskNotes == ["destructive"])
        #expect(snapshot.attention[1].interaction?.questions.first?.options.first?.label == "PR")

        #expect(snapshot.quota.count == 2)
        #expect(snapshot.quota[0].usedPct == 42.5)
        #expect(snapshot.quota[0].limitName == "5 小时")
        #expect(snapshot.quota[1].usedPct == nil)
        #expect(snapshot.quota[1].status == "unavailable")

        #expect(snapshot.stats.eventCount == 12)
        #expect(snapshot.stats.metrics.approvalRequests == 4)
        #expect(snapshot.capabilities?.codexConnector?.managedThreads == 1)
    }

    @Test func decodesSnapshotWebSocketEnvelope() throws {
        let json = """
        {"type":"snapshot","snapshot":\(emptySnapshotJSON)}
        """
        let envelope = try JSONDecoder().decode(SnapshotEnvelope.self, from: Data(json.utf8))
        #expect(envelope.type == "snapshot")
        #expect(envelope.snapshot.sessions.isEmpty)
    }

    @Test func commandRequestEncodesExpectedFieldNames() throws {
        let command = CommandRequest(
            id: UUID(uuidString: "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d")!,
            attentionId: "att-1",
            requestId: UUID(uuidString: "00000000-0000-0000-0000-000000000001"),
            action: AttentionAction.approve.rawValue
        )
        let data = try JSONEncoder().encode(command)
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["attentionId"] as? String == "att-1")
        #expect(object["action"] as? String == "approve")
        #expect(object["requestId"] != nil)
    }

    @Test func settingsEncodeTheCompleteRuntimeContract() throws {
        var settings = UISettings.defaults
        #expect(settings.taskCardFields.contains("tokens"))
        #expect(settings.taskCardFields.contains("cost"))
        #expect(settings.displayFieldsVersion == 2)
        settings.notificationRules.question = .ignore
        settings.providerMuted.codex = true
        settings.retentionDays = 180
        settings.displayProfile = "developer"
        settings.taskCardFields = ["task", "providerTurnId"]

        let data = try JSONEncoder().encode(settings)
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        let rules = try #require(object["notificationRules"] as? [String: Any])
        let muted = try #require(object["providerMuted"] as? [String: Any])
        #expect(rules["question"] as? String == "ignore")
        #expect(muted["codex"] as? Bool == true)
        #expect(object["retentionDays"] as? Int == 180)
        #expect(object["taskCardFields"] as? [String] == ["task", "providerTurnId"])
    }

    @Test func tokenAndEstimatedCostFieldsEncodeIndependently() throws {
        var settings = UISettings.defaults
        settings.displayProfile = "custom"
        settings.taskCardFields = ["tokens"]

        let tokenData = try JSONEncoder().encode(settings)
        let tokenObject = try #require(try JSONSerialization.jsonObject(with: tokenData) as? [String: Any])
        #expect(tokenObject["taskCardFields"] as? [String] == ["tokens"])

        settings.taskCardFields = ["cost"]
        let costData = try JSONEncoder().encode(settings)
        let costObject = try #require(try JSONSerialization.jsonObject(with: costData) as? [String: Any])
        #expect(costObject["taskCardFields"] as? [String] == ["cost"])
    }

    private var emptySnapshotJSON: String {
        """
        {"sessions":[],"attention":[],"commands":[],"quota":[],"stats":{"eventCount":0,"metrics":{"activeDays":0,"approvalRequests":0,"widgetApprovals":0,"widgetDenials":0,"passThroughManual":0,"passThroughTimeout":0,"decisionResponseMsTotal":0,"decisionResponseCount":0,"bannersShown":0,"sessionsObserved":0,"appOpened":0,"todayWidgetDecisions":0}}}
        """
    }
}
