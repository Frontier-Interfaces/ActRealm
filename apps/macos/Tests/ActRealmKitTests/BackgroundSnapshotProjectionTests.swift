import Foundation
import Testing
@testable import ActRealmKit

private func projectionSession(
    id: String,
    execState: String = "thinking",
    lastEventAt: UInt64 = 1_000
) -> SessionRecord {
    SessionRecord(
        id: id,
        provider: "codex",
        providerSessionId: id,
        project: "projection-tests",
        title: "Task \(id)",
        model: nil,
        execState: execState,
        approvalOwner: nil,
        activity: nil,
        activitySince: nil,
        planDone: nil,
        planTotal: nil,
        lastEventAt: lastEventAt
    )
}

private func projectionStats(eventCount: UInt64 = 0) -> SnapshotStats {
    SnapshotStats(
        eventCount: eventCount,
        metrics: MetricsSummary(
            activeDays: 0,
            approvalRequests: 0,
            widgetApprovals: 0,
            widgetDenials: 0,
            passThroughManual: 0,
            passThroughTimeout: 0,
            decisionResponseMsTotal: 0,
            decisionResponseCount: 0,
            bannersShown: 0,
            sessionsObserved: 0,
            appOpened: 0,
            todayWidgetDecisions: 0
        )
    )
}

private func projectionSnapshot(
    sessions: [SessionRecord],
    quota: [QuotaEntry] = [],
    eventCount: UInt64 = 0
) -> Snapshot {
    Snapshot(
        sessions: sessions,
        attention: [],
        commands: [],
        quota: quota,
        stats: projectionStats(eventCount: eventCount)
    )
}

private func projectionQuota(remaining: Double) -> QuotaEntry {
    QuotaEntry(
        provider: "codex",
        window: "5h",
        status: "available",
        usedPct: 100 - remaining,
        remainingPct: remaining,
        resetsAt: nil,
        source: "test",
        capturedAt: 1_000,
        reason: nil
    )
}

@Suite struct NativeTaskProjectionTests {
    @Test func quotaAndMetricOnlySnapshotsDoNotRepublishUnchangedTaskCards() {
        var projector = TaskRenderProjector()
        let tasks = [projectionSession(id: "A")]

        _ = projector.apply(projectionSnapshot(sessions: tasks, quota: [projectionQuota(remaining: 80)]))
        let result = projector.apply(projectionSnapshot(
            sessions: tasks,
            quota: [projectionQuota(remaining: 79)],
            eventCount: 1
        ))

        #expect(result.changedTaskIDs.isEmpty)
        #expect(result.signatures.map(\.sessionID) == ["A"])
    }

    @Test func oneChangedSessionInvalidatesOnlyThatTaskCard() {
        let runningA = LaneTask(session: projectionSession(id: "A"), openAttention: [])
        let waitingA = LaneTask(
            session: projectionSession(id: "A", execState: "awaiting_approval"),
            openAttention: []
        )
        let idleB = LaneTask(
            session: projectionSession(id: "B", execState: "idle"),
            openAttention: []
        )

        let result = TaskRenderProjector.diff(
            old: [runningA, idleB],
            new: [waitingA, idleB]
        )

        #expect(result.changedTaskIDs == ["A"])
    }

    @Test func nativePresentationLatencyKeepsOneHundredNewestSamplesAndUsesP95Index() {
        var latency = NativePresentationLatency()
        let renderedAt = Date(timeIntervalSince1970: 10_000)

        for milliseconds in 0 ... 100 {
            let accepted = latency.record(
                eventAt: renderedAt.addingTimeInterval(-Double(milliseconds) / 1_000),
                renderedAt: renderedAt
            )
            #expect(accepted)
        }

        #expect(latency.sampleCount == 100)
        #expect(latency.p95Milliseconds == 95)
    }

    @Test func nativePresentationLatencyRejectsFutureAndStaleEvents() {
        var latency = NativePresentationLatency()
        let renderedAt = Date(timeIntervalSince1970: 10_000)

        let futureAccepted = latency.record(
            eventAt: renderedAt.addingTimeInterval(0.001),
            renderedAt: renderedAt
        )
        let staleAccepted = latency.record(
            eventAt: renderedAt.addingTimeInterval(-10.001),
            renderedAt: renderedAt
        )
        #expect(!futureAccepted)
        #expect(!staleAccepted)
        #expect(latency.sampleCount == 0)
        #expect(latency.p95Milliseconds == nil)
    }

    @Test func projectsFiveHundredSessionsBelowThreeHundredMillisecondsWithoutWorkspaceFocus() {
        let sessions = (0 ..< 500).map {
            projectionSession(id: "session-\($0)", lastEventAt: UInt64(1_000 + $0))
        }
        let clock = ContinuousClock()
        var samples: [Duration] = []
        var projector = TaskRenderProjector()

        for change in 0 ..< 100 {
            var batch = sessions
            batch[change % batch.count] = projectionSession(
                id: "session-\(change % batch.count)",
                execState: change.isMultiple(of: 2) ? "thinking" : "awaiting_approval",
                lastEventAt: UInt64(2_000 + change)
            )
            let started = clock.now
            _ = projector.apply(projectionSnapshot(sessions: batch, eventCount: UInt64(change)))
            samples.append(started.duration(to: clock.now))
        }

        samples.sort()
        let p95Index = Int(ceil(Double(samples.count) * 0.95)) - 1
        #expect(samples[p95Index] < .milliseconds(300))
    }
}
