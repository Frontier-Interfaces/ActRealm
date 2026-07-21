import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct MenuBarPopoverTests {
    @Test func notificationTonesMatchTheMainOutboxSemantics() {
        #expect(MenuBarStatusTone(kind: .approval) == .amber)
        #expect(MenuBarStatusTone(kind: .nativeApproval) == .amber)
        #expect(MenuBarStatusTone(kind: .question) == .blue)
        #expect(MenuBarStatusTone(kind: .error) == .red)
        #expect(MenuBarStatusTone(kind: .completion) == .green)
    }

    @Test func waitingTaskIsFeaturedAheadOfANewerIdleTask() {
        let snapshot = makeMenuBarSnapshot(sessions: [
            makeMenuBarSession(
                id: "new-idle",
                project: "最近空闲任务",
                execState: "idle",
                lastEventAt: 3_000
            ),
            makeMenuBarSession(
                id: "old-waiting",
                project: "等待处理任务",
                execState: "awaiting_approval",
                lastEventAt: 1_000
            ),
        ])
        let lane = DerivedState.derive(from: snapshot).lanes.first { $0.provider == .claude }!

        let presentation = MenuBarLanePresentation(
            lane: lane,
            now: Date(timeIntervalSince1970: 4)
        )

        #expect(presentation.title == "等待处理任务")
        #expect(presentation.subtitle == "Claude · 等待处理")
        #expect(presentation.trailing == "1 项待处理")
        #expect(presentation.tone == .amber)
    }

    @Test func emptyLaneUsesOneNeutralMessage() {
        let lane = DerivedState.derive(from: makeMenuBarSnapshot()).lanes.first {
            $0.provider == .codex
        }!

        let presentation = MenuBarLanePresentation(lane: lane, now: .now)

        #expect(presentation.title == "Codex")
        #expect(presentation.subtitle == "Codex · 无活动任务")
        #expect(presentation.trailing == "无活动")
        #expect(presentation.tone == .neutral)
    }
}

private func makeMenuBarSession(
    id: String,
    project: String,
    execState: String,
    lastEventAt: UInt64
) -> SessionRecord {
    SessionRecord(
        id: id,
        provider: "claude",
        providerSessionId: id,
        project: project,
        title: "任务 \(id)",
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

private func makeMenuBarSnapshot(sessions: [SessionRecord] = []) -> Snapshot {
    Snapshot(
        sessions: sessions,
        attention: [],
        commands: [],
        quota: [],
        stats: Snapshot.empty.stats
    )
}
