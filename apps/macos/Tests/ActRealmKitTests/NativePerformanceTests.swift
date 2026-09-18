import Foundation
import Testing
@testable import ActRealmKit

private let ms: UInt64 = 1_000_000
private let origin: UInt64 = 100_000 * ms

private func reception(_ stream: UUID, _ sequence: UInt64, delay: UInt64 = 20,
    runtime: String = "runtime-a", at: UInt64 = origin,
    clock: String = "CLOCK_MONOTONIC") -> SnapshotReceiveTiming {
    SnapshotReceiveTiming(streamID: stream, sequence: sequence, receivedAtNs: at,
        delivery: SnapshotDeliveryTiming(runtimeInstanceId: runtime, sequence: sequence,
            clock: clock, startedAtNs: at - delay * ms, readyAtNs: at - ms))
}

@Suite struct NativePerformanceTests {
    @Test func boundedRollingWindowUsesNearestRankP95AndExpiresWhileIdle() {
        var window = LatencyWindow()
        for duration in 0...100 {
            window.record(start: origin - UInt64(duration) * ms, end: origin)
        }
        let summary = window.summary(at: origin)
        #expect(summary.sampleCount == 100)
        #expect(summary.p95Milliseconds == 95)
        let retainedBeforeExpiry = window.summary(at: origin + LatencyWindow.retentionNs - 1).sampleCount == 100
        #expect(retainedBeforeExpiry)
        let expiredAtBoundary = window.summary(at: origin + LatencyWindow.retentionNs) == .empty
        #expect(expiredAtBoundary)
    }

    @Test func genuineSlowSamplesAreNotDiscardedByTheOldTenSecondCeiling() {
        var window = LatencyWindow()
        window.record(start: origin, end: origin + 12_000 * ms)
        let keepsSlowSamples = window.summary(at: origin + 12_000 * ms).p95Milliseconds == 12_000
        #expect(keepsSlowSamples)
        window.record(start: origin + 13_000 * ms, end: origin + 12_000 * ms)
        let rejectsReversedClock = window.summary(at: origin + 12_000 * ms).sampleCount == 1
        #expect(rejectsReversedClock)
    }

    @Test func firstSnapshotOnEveryConnectionIsABaselineNotLiveLatency() {
        var tracker = NativePerformanceTracker()
        let first = UUID()
        let rejectsHTTPInitialization = !tracker.receive(nil)
        #expect(rejectsHTTPInitialization)
        let rejectsInitialFrame = !tracker.receive(reception(first, 1, delay: 9_000))
        #expect(rejectsInitialFrame)
        let acceptsLiveFrame = tracker.receive(reception(first, 2, delay: 20))
        #expect(acceptsLiveFrame)
        let rejectsReconnectBaseline = !tracker.receive(reception(UUID(), 1, delay: 8_000))
        #expect(rejectsReconnectBaseline)
        let summary = tracker.summary(at: origin)
        #expect(summary.delivery.sampleCount == 1)
        #expect(summary.delivery.p95Milliseconds == 20)
    }

    @Test func queuedFramesKeepTheirOwnTimingAndRepeatedOrReorderedFramesDoNotSample() {
        var tracker = NativePerformanceTracker()
        let stream = UUID()
        _ = tracker.receive(reception(stream, 1))
        let older = reception(stream, 2, delay: 110)
        let newer = reception(stream, 3, delay: 15, at: origin + 100 * ms)
        let acceptsOlderFrame = tracker.receive(older)
        #expect(acceptsOlderFrame)
        let acceptsNewerFrame = tracker.receive(newer)
        #expect(acceptsNewerFrame)
        let rejectsDuplicate = !tracker.receive(newer)
        #expect(rejectsDuplicate)
        let rejectsReorderedFrame = !tracker.receive(older)
        #expect(rejectsReorderedFrame)
        let summary = tracker.summary(at: origin + 100 * ms)
        #expect(summary.delivery.sampleCount == 2)
        #expect(summary.delivery.p95Milliseconds == 110)
    }

    @Test func runtimeRestartDiscardsThePreviousRuntimeWindow() {
        var tracker = NativePerformanceTracker()
        let stream = UUID()
        _ = tracker.receive(reception(stream, 1))
        _ = tracker.receive(reception(stream, 2, delay: 7_000))
        tracker.recordProcessing(start: origin - 3 * ms, end: origin, background: false)
        let rejectsRestartBaseline = !tracker.receive(reception(stream, 3, runtime: "runtime-b"))
        #expect(rejectsRestartBaseline)
        let clearsPreviousRuntime = tracker.summary(at: origin) == .empty
        #expect(clearsPreviousRuntime)
        let acceptsNewRuntimeFrame = tracker.receive(reception(stream, 4, delay: 25, runtime: "runtime-b"))
        #expect(acceptsNewRuntimeFrame)
        let usesNewRuntimeWindow = tracker.summary(at: origin).delivery.p95Milliseconds == 25
        #expect(usesNewRuntimeWindow)
    }

    @Test func unknownOrInconsistentRemoteClockDoesNotInventArrivalTiming() {
        var tracker = NativePerformanceTracker()
        let stream = UUID()
        _ = tracker.receive(reception(stream, 1))
        let acceptsLocalWithUnknownClock = tracker.receive(reception(stream, 2, clock: "wall-clock"))
        #expect(acceptsLocalWithUnknownClock)
        let future = SnapshotReceiveTiming(streamID: stream, sequence: 3, receivedAtNs: origin,
            delivery: SnapshotDeliveryTiming(runtimeInstanceId: "runtime-a", sequence: 3,
                clock: "CLOCK_MONOTONIC", startedAtNs: origin + ms, readyAtNs: origin + 2 * ms))
        let acceptsLocalWithInvalidRemoteTime = tracker.receive(future)
        #expect(acceptsLocalWithInvalidRemoteTime)
        let remoteTimingUnavailable = tracker.summary(at: origin).delivery == .empty
        #expect(remoteTimingUnavailable)
        tracker.recordProcessing(start: origin - 3 * ms, end: origin, background: false)
        let localTimingAvailable = tracker.summary(at: origin).processing.p95Milliseconds == 3
        #expect(localTimingAvailable)
    }

    @Test func backgroundWorkHasItsOwnWindowAndNeverPollutesLiveProcessing() {
        var tracker = NativePerformanceTracker()
        tracker.recordProcessing(start: origin - 4 * ms, end: origin, background: false)
        tracker.recordProcessing(start: origin - 3_465 * ms, end: origin, background: true)
        let summary = tracker.summary(at: origin)
        #expect(summary.processing.p95Milliseconds == 4)
        #expect(summary.catchUp.p95Milliseconds == 3_465)
    }

    @Test func legacyRuntimeStillUpdatesAndMeasuresLocalProcessingWithoutInventingTransport() {
        var tracker = NativePerformanceTracker()
        let stream = UUID()
        let initial = tracker.receive(SnapshotReceiveTiming(streamID: stream, sequence: 1,
            receivedAtNs: origin, delivery: nil))
        let live = tracker.receive(SnapshotReceiveTiming(streamID: stream, sequence: 2,
            receivedAtNs: origin + ms, delivery: nil))
        #expect(!initial)
        #expect(live)
        tracker.recordProcessing(start: origin, end: origin + 2 * ms, background: false)
        let summary = tracker.summary(at: origin + 2 * ms)
        #expect(summary.delivery == .empty)
        #expect(summary.processing.p95Milliseconds == 2)
    }

    @Test func malformedOptionalTimingCannotDropTasksFromAnOtherwiseValidSnapshot() throws {
        let snapshot = try JSONSerialization.jsonObject(with: JSONEncoder().encode(Snapshot.empty))
        for timing in [NSNull(), ["clock": "future-format"]] as [Any] {
            let data = try JSONSerialization.data(withJSONObject: ["type": "snapshot",
                "snapshot": snapshot, "deliveryTiming": timing])
            let decoded = try JSONDecoder().decode(SnapshotEnvelope.self, from: data)
            #expect(decoded.snapshot == .empty)
            #expect(decoded.deliveryTiming == nil)
        }
    }

    @Test @MainActor func globalCounterGrowthAndOldSessionTimesDoNotBecomeLatency() {
        let suite = "ActRealmLatency.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        defer { model.shutdown() }
        let stream = UUID()
        func snapshot(_ count: UInt64) -> Snapshot {
            // Reproduces the reported case: only a global counter changes.
            // No visible session timestamp advances, including archived events.
            let session = SessionRecord(id: "same-visible-task", provider: "codex",
                providerSessionId: "same-visible-task", project: "fixture", title: "Stable task",
                model: nil, execState: "thinking", approvalOwner: nil, activity: nil,
                activitySince: nil, planDone: nil, planTotal: nil, lastEventAt: 1_000)
            return Snapshot(sessions: [session], attention: [], commands: [], quota: [],
                stats: SnapshotStats(eventCount: count, metrics: Snapshot.empty.stats.metrics))
        }
        let baseline = reception(stream, 1, at: PerformanceClock.now())
        model.receive(update: SnapshotUpdate(snapshot: snapshot(100), timing: baseline))
        #expect(model.performanceMetrics == .empty)
        let current = reception(stream, 2, delay: 5, at: PerformanceClock.now())
        model.receive(update: SnapshotUpdate(snapshot: snapshot(101), timing: current))
        #expect(model.performanceMetrics.delivery.p95Milliseconds == 5)
        #expect(model.performanceMetrics.processing.sampleCount == 1)
        let live = model.performanceMetrics.processing
        model.updateWorkspaceAnimationActive(false)
        model.receive(update: SnapshotUpdate(snapshot: snapshot(102),
            timing: reception(stream, 3, at: PerformanceClock.now())))
        #expect(model.performanceMetrics.processing == live)
        model.updateWorkspaceAnimationActive(true)
        #expect(model.performanceMetrics.processing == live)
        #expect(model.performanceMetrics.catchUp.sampleCount == 1)
    }
}
