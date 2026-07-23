import AppKit
import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct MenuBarPopoverTests {
    @Test @MainActor func menuBarMarkUsesAFixedTemplateImage() {
        #expect(MenuBarMark.templateImage.size == NSSize(width: 16, height: 14))
        #expect(MenuBarMark.templateImage.isTemplate)
    }

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

        #expect(presentation.title == "Claude")
        #expect(presentation.subtitle == "Claude · 等待处理")
        #expect(presentation.trailing == "1 项待处理")
        #expect(presentation.tone == .amber)
    }

    @Test func activeLaneTitleUsesTheAgentNameInsteadOfTheProjectDirectory() {
        let snapshot = makeMenuBarSnapshot(sessions: [
            makeMenuBarSession(
                id: "codex-running",
                provider: "codex",
                project: "jian",
                execState: "tool_running",
                lastEventAt: 3_000
            ),
        ])
        let lane = DerivedState.derive(from: snapshot).lanes.first { $0.provider == .codex }!

        let presentation = MenuBarLanePresentation(lane: lane, now: .now)

        #expect(presentation.title == "Codex")
        #expect(presentation.trailing == "1 项运行中")
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
    provider: String = "claude",
    project: String,
    execState: String,
    lastEventAt: UInt64
) -> SessionRecord {
    SessionRecord(
        id: id,
        provider: provider,
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
