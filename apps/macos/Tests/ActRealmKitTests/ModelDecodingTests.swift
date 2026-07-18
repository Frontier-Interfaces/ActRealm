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
              "project": "flow-agent",
              "title": "Refactor server auth",
              "model": "claude-sonnet-5",
              "execState": "running",
              "approvalOwner": null,
              "activity": "Editing files",
              "activitySince": 1737000000000,
              "planDone": 2,
              "planTotal": 5,
              "inputTokens": 48000,
              "outputTokens": 1300,
              "totalTokens": 49300,
              "contextWindowTokens": 258400,
              "usageCapturedAt": 1737000090000,
              "lastEventAt": 1737000100000
            }
          ],
          "attention": [
            {
              "id": "att-1",
              "sessionId": "sess-1",
              "provider": "claude",
              "project": "flow-agent",
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
              "resetsAt": 1737003600000,
              "source": "statusline",
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
          }
        }
        """

        let snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8))

        #expect(snapshot.sessions.count == 1)
        #expect(snapshot.sessions[0].providerSessionId == "abc123")
        #expect(snapshot.sessions[0].planDone == 2)
        #expect(snapshot.sessions[0].totalTokens == 49_300)
        #expect(snapshot.sessions[0].contextWindowTokens == 258_400)

        #expect(snapshot.attention.count == 1)
        #expect(snapshot.attention[0].kind == "approval")
        #expect(snapshot.attention[0].requestId?.uuidString.lowercased() == "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d")
        #expect(snapshot.attention[0].riskNotes == ["destructive"])

        #expect(snapshot.quota.count == 2)
        #expect(snapshot.quota[0].usedPct == 42.5)
        #expect(snapshot.quota[1].usedPct == nil)
        #expect(snapshot.quota[1].status == "unavailable")

        #expect(snapshot.stats.eventCount == 12)
        #expect(snapshot.stats.metrics.approvalRequests == 4)
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

    private var emptySnapshotJSON: String {
        """
        {"sessions":[],"attention":[],"commands":[],"quota":[],"stats":{"eventCount":0,"metrics":{"activeDays":0,"approvalRequests":0,"widgetApprovals":0,"widgetDenials":0,"passThroughManual":0,"passThroughTimeout":0,"decisionResponseMsTotal":0,"decisionResponseCount":0,"bannersShown":0,"sessionsObserved":0,"appOpened":0,"todayWidgetDecisions":0}}}
        """
    }
}
