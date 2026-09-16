import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

private func usageSession(at: UInt64 = 2_000, turn: UInt64 = 1_000) -> SessionRecord {
    SessionRecord(id: "usage-task", provider: "codex", providerSessionId: "usage-task", project: "fixture", title: "Usage task", model: nil, execState: "thinking", approvalOwner: nil, activity: nil, activitySince: nil, planDone: nil, planTotal: nil, turnStartedAt: turn, lastEventAt: at)
}

private func usageSnapshot(session: SessionRecord = usageSession(), total: UInt64 = 100, attention: [AttentionRecord] = []) -> Snapshot {
    Snapshot(sessions: [session], attention: attention, commands: [], quota: [], tokenUsage: TokenUsageTotals(today: total, month: total, total: total, collectionState: "partial", dataQuality: "partial"), stats: Snapshot.empty.stats)
}

@Suite @MainActor struct UsageProductRegressionTests {
    @Test func partialOrTemporarilyUnavailableHistoryDoesNotRemoveObservedAnalytics() {
        for state in ["partial", "scanning", "unavailable"] {
            let totals = TokenUsageTotals(today: 100, month: 200, total: 300, collectionState: state, dataQuality: state)
            #expect(TokenDashboardPresentation.hasObservedData(totals))
            #expect(!TokenDashboardPresentation.analyticsAreFinal(totals))
        }
        #expect(!TokenDashboardPresentation.hasObservedData(.empty))
        #expect(TokenDashboardPresentation.hasObservedData(TokenUsageTotals(today: 0, month: 0, total: 0, collectionState: "ready", dataQuality: "verified")))
    }

    @Test func backgroundWorkspaceStillPublishesLiveUsageForTheDashboard() {
        let suite = "ActRealmUsageLive.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        defer { model.shutdown() }
        model.receive(snapshot: usageSnapshot(total: 100))
        model.updateWorkspaceAnimationActive(false)
        let cards = model.taskRenderSignatures
        model.receive(snapshot: usageSnapshot(total: 300))
        #expect(model.tokenUsage.total == 300)
        #expect(model.tokenUsage.dataQuality == "partial")
        #expect(model.taskRenderSignatures == cards)
    }

    @Test func deleteActiveCardPersistsWithoutChangingUsageAndOnlyNewEventsRestoreIt() {
        let suite = "ActRealmCardDeletion.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        defer { model.shutdown() }
        let snapshot = usageSnapshot()
        model.receive(snapshot: snapshot)
        let task = LaneTask(session: snapshot.sessions[0], openAttention: [])
        model.deleteTaskCard(task, at: 3_000)
        #expect(model.visibleAgentTasks.isEmpty)
        #expect(model.tokenUsage.total == 100)
        #expect(model.derived.agentTasks.contains(where: { $0.id == task.id }))
        model.receive(snapshot: usageSnapshot(total: 200))
        #expect(model.isTaskDismissed(task))
        let relaunched = AppModel(defaults: defaults, demo: true)
        #expect(relaunched.isTaskDismissed(task))
        let newer = LaneTask(session: usageSession(at: 3_001), openAttention: [])
        #expect(!model.isTaskDismissed(newer))
        let newerTurn = LaneTask(session: usageSession(at: 2_000, turn: 3_001), openAttention: [])
        #expect(!model.isTaskDismissed(newerTurn))
    }

    @Test func deletingAPendingCardNeverAnswersOrHidesItsOutboxRequest() {
        let suite = "ActRealmPendingCardDeletion.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        defer { model.shutdown() }
        let approval = AttentionRecord(id: "approval", sessionId: "usage-task", provider: "codex", project: "fixture", requestId: UUID(), kind: "approval", title: "Approve?", detail: nil, state: "open", risk: "low", riskNotes: [], commandPreview: nil, expiresAt: nil, createdAt: 2_000, resolution: nil)
        let snapshot = usageSnapshot(attention: [approval])
        model.receive(snapshot: snapshot)
        let task = LaneTask(session: snapshot.sessions[0], openAttention: [approval])
        model.deleteTaskCard(task, at: 3_000)
        #expect(model.visibleAgentTasks.isEmpty)
        #expect(model.derived.openOutbox.map(\.id) == ["approval"])
        #expect(model.derived.pendingDecision == nil)
    }
}
