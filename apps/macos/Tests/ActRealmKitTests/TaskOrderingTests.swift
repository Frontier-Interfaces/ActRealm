import ActRealmKit
@testable import ActRealmUI
import Foundation
import Testing

@Suite("Agent task presentation ordering")
struct TaskOrderingTests {
    private struct Candidate {
        let id: String
        let status: LaneTaskStatus
        let priority: Int
        let oldestOpenOutboxAt: Date?
        let turnStartedAt: Date?
        let lastEventAt: Date
        let isPinned: Bool

        init(
            id: String,
            status: LaneTaskStatus,
            priority: Int? = nil,
            oldestOpenOutboxAt: Date? = nil,
            turnStartedAt: Date? = nil,
            lastEventAt: Date,
            isPinned: Bool = false
        ) {
            self.id = id
            self.status = status
            if let priority {
                self.priority = priority
            } else {
                self.priority = switch status {
                case .failed: 0
                case .waiting: 1
                case .running: 4
                case .done: 5
                case .idle: 6
                }
            }
            self.oldestOpenOutboxAt = oldestOpenOutboxAt
            self.turnStartedAt = turnStartedAt
            self.lastEventAt = lastEventAt
            self.isPinned = isPinned
        }
    }

    @Test("actionable and failed tasks stay ahead of active and inactive work")
    func actionableStatePrioritySurvivesRecency() {
        let candidates = [
            Candidate(id: "idle-newest", status: .idle, lastEventAt: date(500)),
            Candidate(id: "done", status: .done, lastEventAt: date(400)),
            Candidate(id: "running", status: .running, turnStartedAt: date(100), lastEventAt: date(300)),
            Candidate(id: "failed", status: .failed, lastEventAt: date(200)),
            Candidate(id: "waiting-oldest", status: .waiting, oldestOpenOutboxAt: date(10), lastEventAt: date(100))
        ]

        let ordered = AgentTaskPresentationOrdering.ordered(
            candidates,
            status: \.status,
            priority: \.priority,
            oldestOpenOutboxAt: \.oldestOpenOutboxAt,
            turnStartedAt: \.turnStartedAt,
            lastEventAt: \.lastEventAt,
            isPinned: \.isPinned,
            id: \.id
        )

        #expect(ordered.map(\.id) == [
            "failed",
            "waiting-oldest",
            "running",
            "done",
            "idle-newest"
        ])
    }

    @Test("oldest user wait is handled first")
    func oldestWaitUsesFairQueueOrder() {
        let candidates = [
            Candidate(id: "new-wait", status: .waiting, oldestOpenOutboxAt: date(20), lastEventAt: date(90)),
            Candidate(id: "old-wait", status: .waiting, oldestOpenOutboxAt: date(10), lastEventAt: date(30))
        ]

        let ordered = AgentTaskPresentationOrdering.ordered(
            candidates,
            status: \.status,
            priority: \.priority,
            oldestOpenOutboxAt: \.oldestOpenOutboxAt,
            turnStartedAt: \.turnStartedAt,
            lastEventAt: \.lastEventAt,
            isPinned: \.isPinned,
            id: \.id
        )

        #expect(ordered.map(\.id) == ["old-wait", "new-wait"])
    }

    @Test("direct approval precedes Provider-native approval and questions")
    func attentionKindsUseTheProductPriority() {
        let candidates = [
            Candidate(id: "question", status: .waiting, priority: 3, oldestOpenOutboxAt: date(1), lastEventAt: date(1)),
            Candidate(id: "native", status: .waiting, priority: 2, oldestOpenOutboxAt: date(2), lastEventAt: date(2)),
            Candidate(id: "direct", status: .waiting, priority: 1, oldestOpenOutboxAt: date(3), lastEventAt: date(3))
        ]

        #expect(orderedIDs(candidates) == ["direct", "native", "question"])
    }

    @Test("tool events do not reshuffle running turns")
    func runningTasksUseTurnStartInsteadOfLatestToolEvent() {
        let before = [
            Candidate(id: "older-turn", status: .running, turnStartedAt: date(100), lastEventAt: date(900)),
            Candidate(id: "newer-turn", status: .running, turnStartedAt: date(200), lastEventAt: date(300))
        ]
        let afterToolEvent = [
            Candidate(id: "older-turn", status: .running, turnStartedAt: date(100), lastEventAt: date(2_000)),
            before[1]
        ]

        #expect(orderedIDs(before) == ["newer-turn", "older-turn"])
        #expect(orderedIDs(afterToolEvent) == ["newer-turn", "older-turn"])
    }

    @Test("expanded running task stays first inside the running group")
    func pinnedRunningTaskKeepsItsPosition() {
        let candidates = [
            Candidate(id: "newer", status: .running, turnStartedAt: date(200), lastEventAt: date(300)),
            Candidate(id: "expanded", status: .running, turnStartedAt: date(100), lastEventAt: date(150), isPinned: true)
        ]

        #expect(orderedIDs(candidates) == ["expanded", "newer"])
    }

    @Test("equal facts use stable session identity")
    func equalFactsUseStableID() {
        let candidates = [
            Candidate(id: "later-id", status: .failed, lastEventAt: date(50)),
            Candidate(id: "newest", status: .failed, lastEventAt: date(70)),
            Candidate(id: "earlier-id", status: .failed, lastEventAt: date(50))
        ]

        #expect(orderedIDs(candidates) == ["newest", "earlier-id", "later-id"])
    }

    private func orderedIDs(_ candidates: [Candidate]) -> [String] {
        AgentTaskPresentationOrdering.ordered(
            candidates,
            status: \.status,
            priority: \.priority,
            oldestOpenOutboxAt: \.oldestOpenOutboxAt,
            turnStartedAt: \.turnStartedAt,
            lastEventAt: \.lastEventAt,
            isPinned: \.isPinned,
            id: \.id
        ).map(\.id)
    }

    private func date(_ seconds: TimeInterval) -> Date {
        Date(timeIntervalSince1970: seconds)
    }
}
