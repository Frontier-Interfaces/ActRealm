import Foundation
import Testing
@testable import ActRealmKit

private let approvalTestNow = Date(timeIntervalSince1970: 100)
private let approvalRequestID = UUID(uuidString: "01980000-0000-7000-8000-000000000001")!

private func approvalRecord(
    actions: [String]? = ["approve", "deny"],
    remote: Bool? = true,
    requestID: UUID? = approvalRequestID,
    kind: String = "approval",
    state: String = "open",
    risk: String = "high",
    expiry: UInt64? = 200_000
) -> AttentionRecord {
    AttentionRecord(
        id: "approval", sessionId: "task", provider: "claude", project: "fixture",
        requestId: requestID, kind: kind, title: "Allow Bash?", detail: nil,
        state: state, risk: risk, riskNotes: ["High impact"], primaryCategory: "git.push",
        commandPreview: "git push <redacted>", expiresAt: expiry, createdAt: 90_000,
        resolution: nil, remoteActionable: remote, allowedActions: actions
    )
}

private func approvalEntry(_ record: AttentionRecord) -> OutboxEntry {
    OutboxEntry(attention: record, sessionTitle: "Fixture")
}

@Suite struct ApprovalCapabilityTests {
    @Test func allRiskLevelsUseDeclaredActionsInsteadOfGitCommandWhitelist() {
        for risk in ["low", "med", "high", "unknown"] {
            let entry = approvalEntry(approvalRecord(risk: risk))
            #expect(entry.approvalActions(at: approvalTestNow, connectionIsLive: true) == ["approve", "deny"])
        }
    }

    @Test func explicitEmptyAndDenyOnlyDeclarationsOverrideLegacyCapability() {
        let cases: [([String], Set<String>)] = [
            ([], []), (["deny"], ["deny"]), (["approve"], ["approve"]),
            (["future-grant"], []), (["deny", "future-grant"], ["deny"]),
        ]
        for (actions, expected) in cases {
            let entry = approvalEntry(approvalRecord(actions: actions, remote: true))
            #expect(entry.approvalActions(at: approvalTestNow, connectionIsLive: true) == expected)
        }
    }

    @Test func olderRuntimeFallbackMatchesDisplayWithoutInventingAllow() {
        #expect(approvalEntry(approvalRecord(actions: nil, remote: true))
            .approvalActions(at: approvalTestNow, connectionIsLive: true) == ["approve", "deny"])
        for remote: Bool? in [nil, false] {
            #expect(approvalEntry(approvalRecord(actions: nil, remote: remote))
                .approvalActions(at: approvalTestNow, connectionIsLive: true) == ["deny"])
        }
    }

    @Test func nativeObservationMissingReplyExpiredAndPendingRequestsCannotBeDecided() {
        let records = [
            approvalRecord(requestID: nil), approvalRecord(kind: "native_approval"),
            approvalRecord(kind: "question"), approvalRecord(state: "committing"),
            approvalRecord(state: "decision_sent"), approvalRecord(state: "resolved"),
            approvalRecord(expiry: nil), approvalRecord(expiry: 100_000),
            approvalRecord(expiry: 99_999),
        ]
        for record in records {
            #expect(approvalEntry(record).approvalActions(at: approvalTestNow, connectionIsLive: true).isEmpty)
        }
        #expect(approvalEntry(approvalRecord()).approvalActions(at: approvalTestNow, connectionIsLive: false).isEmpty)
    }

    @Test func snapshotDecodingRetainsExplicitEmptyActions() throws {
        var json = try #require(JSONSerialization.jsonObject(with: JSONEncoder().encode(approvalRecord())) as? [String: Any])
        json["allowedActions"] = [] as [String]
        let decoded = try JSONDecoder().decode(AttentionRecord.self, from: JSONSerialization.data(withJSONObject: json))
        #expect(decoded.allowedActions == [])
        #expect(approvalEntry(decoded).approvalActions(at: approvalTestNow, connectionIsLive: true).isEmpty)
        json.removeValue(forKey: "allowedActions")
        let legacy = try JSONDecoder().decode(AttentionRecord.self, from: JSONSerialization.data(withJSONObject: json))
        #expect(legacy.allowedActions == nil)
    }

    @Test @MainActor func modelRevalidatesAnOldButtonAgainstLatestCapabilitiesAndRequestID() {
        let suite = "ActRealmApprovalCapabilities.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        defer { model.shutdown() }
        func receive(_ record: AttentionRecord?) {
            model.receive(snapshot: Snapshot(sessions: [], attention: record.map { [$0] } ?? [], commands: [], quota: [], stats: Snapshot.empty.stats))
        }
        let old = approvalEntry(approvalRecord())
        receive(old.attention)
        #expect(model.approvalActions(for: old, at: approvalTestNow) == ["approve", "deny"])
        receive(approvalRecord(actions: []))
        #expect(model.approvalActions(for: old, at: approvalTestNow).isEmpty)
        receive(approvalRecord(requestID: UUID()))
        #expect(model.approvalActions(for: old, at: approvalTestNow).isEmpty)
        receive(nil)
        #expect(model.approvalActions(for: old, at: approvalTestNow).isEmpty)
    }
}
