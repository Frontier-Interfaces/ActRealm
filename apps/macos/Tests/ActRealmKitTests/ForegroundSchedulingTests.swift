import Foundation
import Testing
@testable import ActRealmKit

@Suite struct ForegroundSchedulingTests {
    @MainActor
    @Test func defaultsMatchRecommendedReferencePolicy() {
        let model = AppModel(defaults: isolatedDefaults(), demo: false)

        #expect(model.foregroundScheduling.isEnabled)
        #expect(model.foregroundScheduling.closesAfterAcceptance)
        #expect(model.foregroundScheduling.strategy == .remind)
        #expect(model.foregroundScheduling.reminderSeconds == 10)
        #expect(model.foregroundScheduling.returnsToActRealmWorkspace)
        #expect(model.foregroundScheduling.acceptanceSeconds == 10)
        #expect(model.foregroundScheduling.codexRule == .defaultRule)
        #expect(model.foregroundScheduling.claudeRule == .immediate)
    }

    @MainActor
    @Test func editsPersistAndDurationsStayOnSupportedValues() {
        let suite = "ForegroundSchedulingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let first = AppModel(defaults: defaults, demo: false)
        first.updateForegroundScheduling {
            $0.strategy = .immediate
            $0.reminderSeconds = 18
            $0.acceptanceSeconds = 29
            $0.codexRule = .actRealmWorkspace
        }

        #expect(first.foregroundScheduling.reminderSeconds == 10)
        #expect(first.foregroundScheduling.acceptanceSeconds == 30)

        let restored = AppModel(defaults: defaults, demo: false)
        #expect(restored.foregroundScheduling.strategy == .immediate)
        #expect(restored.foregroundScheduling.reminderSeconds == 10)
        #expect(restored.foregroundScheduling.acceptanceSeconds == 30)
        #expect(restored.foregroundScheduling.codexRule == .actRealmWorkspace)
    }

    @MainActor
    @Test func liveArrivalMovesThroughReminderOpenWaitAndReturn() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        let start = Date(timeIntervalSince1970: 1_000)

        model.receiveForegroundArrival(
            id: "approval-1",
            provider: .codex,
            title: "Codex 想运行 Bash，等你批准",
            taskTitle: "修复构建",
            at: start
        )
        #expect(model.foregroundDispatch?.phase == .reminding)
        #expect(model.isForegroundHUDSuppressed(for: "approval-1"))

        model.advanceForegroundDispatch(at: start.addingTimeInterval(10))
        #expect(model.foregroundDispatch?.phase == .opening)

        model.advanceForegroundDispatch(at: start.addingTimeInterval(12))
        #expect(model.foregroundDispatch?.phase == .awaitingWorkspace)

        model.advanceForegroundDispatch(at: start.addingTimeInterval(23))
        #expect(model.foregroundDispatch?.phase == .returnedToActRealmWorkspace)
        #expect(model.foregroundReturnNotes["approval-1"] == "未检测到进入调度工作桌面，已返回")

        model.advanceForegroundDispatch(at: start.addingTimeInterval(25))
        #expect(model.foregroundDispatch == nil)
    }

    @MainActor
    @Test func providerOverrideAndManualReceiveAreApplied() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        #expect(model.effectiveForegroundStrategy(for: .claude) == .immediate)
        #expect(model.effectiveForegroundStrategy(for: .codex) == .remind)

        model.receiveForegroundArrival(
            id: "question-1",
            provider: .claude,
            title: "Claude 有一个问题需要你回答",
            taskTitle: nil
        )
        #expect(model.foregroundDispatch?.phase == .opening)

        model.keepForegroundTaskInActRealmWorkspace()
        #expect(model.foregroundDispatch == nil)
    }

    @MainActor
    @Test func enteringSchedulingWorkspaceStopsReturnAndHonorsAutoClose() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        let start = Date(timeIntervalSince1970: 2_000)
        model.receiveForegroundArrival(
            id: "approval-accepted",
            provider: .codex,
            title: "需要批准",
            taskTitle: nil,
            at: start
        )
        model.advanceForegroundDispatch(at: start.addingTimeInterval(10))
        model.advanceForegroundDispatch(at: start.addingTimeInterval(12))
        #expect(model.foregroundDispatch?.phase == .awaitingWorkspace)

        model.acceptForegroundWorkspace()
        #expect(model.foregroundDispatch == nil)
        #expect(!model.foregroundScheduling.isEnabled)
        #expect(model.foregroundReturnNotes["approval-accepted"] == nil)
    }

    private func isolatedDefaults() -> UserDefaults {
        let suite = "ForegroundSchedulingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }
}
