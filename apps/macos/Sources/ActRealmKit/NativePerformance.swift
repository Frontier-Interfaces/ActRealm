import Darwin
import Foundation

/// Both processes use the same host's POSIX monotonic clock. No Provider event
/// time, global event count, wall-clock change or session list is a timer source.
enum PerformanceClock {
    static func now() -> UInt64 {
        var value = timespec()
        guard clock_gettime(CLOCK_MONOTONIC, &value) == 0,
              value.tv_sec >= 0, value.tv_nsec >= 0 else { return 0 }
        return UInt64(value.tv_sec) * 1_000_000_000 + UInt64(value.tv_nsec)
    }
}

struct SnapshotDeliveryTiming: Codable, Equatable, Sendable {
    let runtimeInstanceId: String
    let sequence: UInt64
    let clock: String
    let startedAtNs: UInt64
    let readyAtNs: UInt64
}

/// Immutable context travels WITH the decoded snapshot through the main queue.
/// Reading a separate "last received" timestamp would pair different frames.
struct SnapshotReceiveTiming: Sendable {
    let streamID: UUID
    let sequence: UInt64
    let receivedAtNs: UInt64
    let delivery: SnapshotDeliveryTiming?
}

struct SnapshotUpdate: Sendable {
    let snapshot: Snapshot
    var timing: SnapshotReceiveTiming? = nil
}

public struct LatencySummary: Equatable, Sendable {
    public let sampleCount: Int
    public let p95Milliseconds: Double?
    public static let empty = Self(sampleCount: 0, p95Milliseconds: nil)
}

public struct NativePerformanceSummary: Equatable, Sendable {
    public let delivery: LatencySummary
    public let processing: LatencySummary
    public let catchUp: LatencySummary
    public static let empty = Self(delivery: .empty, processing: .empty, catchUp: .empty)
}

struct LatencyWindow: Sendable {
    static let maximumSamples = 100
    static let retentionNs: UInt64 = 300 * 1_000_000_000
    private var samples: [(duration: UInt64, recordedAt: UInt64)] = []

    mutating func record(start: UInt64, end: UInt64) {
        prune(at: end)
        guard start > 0, end >= start else { return }
        // A genuinely slow live sample is retained, including durations >10s.
        samples.append((end - start, end))
        if samples.count > Self.maximumSamples {
            samples.removeFirst(samples.count - Self.maximumSamples)
        }
    }

    mutating func summary(at now: UInt64) -> LatencySummary {
        prune(at: now)
        let sorted = samples.map(\.duration).sorted()
        guard !sorted.isEmpty else { return .empty }
        let index = Int(ceil(Double(sorted.count) * 0.95)) - 1
        return LatencySummary(sampleCount: sorted.count,
            p95Milliseconds: Double(sorted[index]) / 1_000_000)
    }

    private mutating func prune(at now: UInt64) {
        samples.removeAll { now < $0.recordedAt || now - $0.recordedAt >= Self.retentionNs }
    }
}

struct NativePerformanceTracker: Sendable {
    private var delivery = LatencyWindow()
    private var processing = LatencyWindow()
    private var catchUp = LatencyWindow()
    private var streamID: UUID?
    private var sequence: UInt64 = 0
    private var runtimeID: String?
    private var remoteSequence: UInt64 = 0

    /// Returns whether this reception belongs to the live processing window.
    /// Initial HTTP loads have no timing; every WebSocket starts with a baseline.
    mutating func receive(_ timing: SnapshotReceiveTiming?) -> Bool {
        guard let timing else { return false }
        let newStream = streamID != timing.streamID
        if newStream {
            streamID = timing.streamID
            sequence = 0
            remoteSequence = 0
        }
        guard timing.sequence > sequence else { return false }
        sequence = timing.sequence
        if let remote = timing.delivery {
            let changedRuntime = runtimeID != nil && runtimeID != remote.runtimeInstanceId
            if changedRuntime {
                delivery = LatencyWindow()
                processing = LatencyWindow()
                catchUp = LatencyWindow()
                remoteSequence = 0
            }
            runtimeID = remote.runtimeInstanceId
            guard remote.sequence > remoteSequence else { return false }
            remoteSequence = remote.sequence
            guard !newStream, !changedRuntime, timing.sequence > 1 else { return false }
            if remote.clock == "CLOCK_MONOTONIC",
               remote.startedAtNs > 0,
               remote.readyAtNs >= remote.startedAtNs,
               timing.receivedAtNs >= remote.readyAtNs {
                delivery.record(start: remote.startedAtNs, end: timing.receivedAtNs)
            }
        } else if newStream || timing.sequence == 1 {
            return false
        }
        return true
    }

    mutating func recordProcessing(start: UInt64, end: UInt64, background: Bool) {
        if background { catchUp.record(start: start, end: end) }
        else { processing.record(start: start, end: end) }
    }

    mutating func summary(at now: UInt64) -> NativePerformanceSummary {
        NativePerformanceSummary(delivery: delivery.summary(at: now),
            processing: processing.summary(at: now), catchUp: catchUp.summary(at: now))
    }
}
