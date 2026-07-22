import Foundation
import Testing
@testable import ActRealmKit

@Suite struct ForegroundSchedulingTests {
    @MainActor
    @Test func defaultsMatchRecommendedReferencePolicy() {
        let model = AppModel(defaults: isolatedDefaults(), demo: false)

        #expect(model.foregroundScheduling.isEnabled)
        #expect(model.foregroundScheduling.eventRules.approval)
        #expect(model.foregroundScheduling.eventRules.question)
        #expect(model.foregroundScheduling.eventRules.error)
        #expect(!model.foregroundScheduling.eventRules.completion)
        #expect(model.foregroundScheduling.strategy == .remind)
        #expect(model.foregroundScheduling.reminderSeconds == 10)
        #expect(model.foregroundScheduling.allowsStageManager)
        #expect(model.foregroundScheduling.stageManagerRestoreTiming == .onReturnToActRealm)
        #expect(model.foregroundScheduling.returnsToActRealmWorkspace)
        #expect(model.foregroundScheduling.acceptanceSeconds == 10)
    }

    @MainActor
    @Test func legacyInvisibleAutoApprovalPreferenceIsRemoved() {
        let defaults = isolatedDefaults()
        defaults.set("allowAll", forKey: "actrealm.approvalPolicy")

        _ = AppModel(defaults: defaults, demo: false)

        #expect(defaults.object(forKey: "actrealm.approvalPolicy") == nil)
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
            $0.eventRules.completion = true
            $0.allowsStageManager = false
            $0.stageManagerRestoreTiming = .afterAcceptance
        }

        #expect(first.foregroundScheduling.reminderSeconds == 10)
        #expect(first.foregroundScheduling.acceptanceSeconds == 30)

        let restored = AppModel(defaults: defaults, demo: false)
        #expect(restored.foregroundScheduling.strategy == .immediate)
        #expect(restored.foregroundScheduling.reminderSeconds == 10)
        #expect(restored.foregroundScheduling.acceptanceSeconds == 30)
        #expect(restored.foregroundScheduling.eventRules.completion)
        #expect(!restored.foregroundScheduling.allowsStageManager)
        #expect(restored.foregroundScheduling.stageManagerRestoreTiming == .afterAcceptance)
    }

    @MainActor
    @Test func liveArrivalMovesThroughReminderOpenWaitAndReturn() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        let start = Date(timeIntervalSince1970: 1_000)
        bindWorkspace(model)

        model.receiveForegroundArrival(
            id: "approval-1",
            provider: .codex,
            eventKind: .approval,
            title: "Codex 请求运行 Bash，等待批准",
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
        #expect(model.foregroundReturnNotes["approval-1"] == "未检测到鼠标进入对应 Agent 的绑定工作区，已返回 ActRealm")

        model.advanceForegroundDispatch(at: start.addingTimeInterval(25))
        #expect(model.foregroundDispatch == nil)
    }

    @MainActor
    @Test func disabledEventTypesStayInActRealmWithoutStartingFocus() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        bindWorkspace(model)

        model.receiveForegroundArrival(
            id: "completion-1",
            provider: .claude,
            eventKind: .completion,
            title: "Claude 已完成任务",
            taskTitle: nil
        )
        #expect(model.foregroundDispatch == nil)

        model.receiveForegroundArrival(
            id: "question-1",
            provider: .claude,
            eventKind: .question,
            title: "Claude 发出一个待回答问题",
            taskTitle: nil
        )
        #expect(model.foregroundDispatch?.phase == .reminding)
    }

    @MainActor
    @Test func closingAgentFocusHUDCancelsTheHiddenSwitchButKeepsTheEvent() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        bindWorkspace(model)

        model.receiveForegroundArrival(
            id: "approval-close",
            provider: .codex,
            eventKind: .approval,
            title: "Codex 请求运行 Bash，等待批准",
            taskTitle: "验证关闭 HUD"
        )
        #expect(model.foregroundDispatch?.phase == .reminding)

        model.dismissHUD()

        #expect(model.foregroundDispatch == nil)
        #expect(model.toastMessage == "已稍后处理；事件仍保留在 ActRealm")
    }

    @MainActor
    @Test func enteringSchedulingWorkspaceStopsReturnWithoutDisablingFocus() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        let start = Date(timeIntervalSince1970: 2_000)
        bindWorkspace(model)
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
        #expect(model.foregroundScheduling.isEnabled)
        #expect(model.foregroundReturnNotes["approval-accepted"] == nil)
    }

    @MainActor
    @Test func unboundWorkspaceKeepsArrivalInListAndDoesNotSuppressHUD() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)

        model.receiveForegroundArrival(
            id: "approval-unbound",
            provider: .codex,
            title: "等待批准",
            taskTitle: nil
        )

        #expect(model.foregroundDispatch == nil)
        #expect(!model.isForegroundHUDSuppressed(for: "approval-unbound"))
        #expect(model.toastMessage?.contains("尚未绑定到工作区") == true)
    }

    @MainActor
    @Test func masterSwitchPreventsHUDAndAutomaticFocus() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        bindWorkspace(model)
        model.updateForegroundScheduling { $0.isEnabled = false }

        model.receiveForegroundArrival(
            id: "approval-disabled",
            provider: .codex,
            eventKind: .approval,
            title: "等待批准",
            taskTitle: nil
        )

        #expect(model.foregroundDispatch == nil)
        #expect(model.foregroundQueuedCount == 0)
        #expect(!model.isForegroundHUDSuppressed(for: "approval-disabled"))
    }

    @MainActor
    @Test func focusEventsAreSerializedAndResolvedReminderCancelsImmediately() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        let start = Date(timeIntervalSince1970: 4_000)
        bindWorkspace(model)

        model.receiveForegroundArrival(
            id: "approval-first",
            provider: .codex,
            eventKind: .approval,
            title: "等待批准",
            taskTitle: nil,
            at: start
        )
        model.receiveForegroundArrival(
            id: "question-second",
            provider: .claude,
            eventKind: .question,
            title: "需要回答",
            taskTitle: nil,
            at: start.addingTimeInterval(1)
        )

        #expect(model.foregroundDispatch?.id == "approval-first")
        #expect(model.foregroundQueuedCount == 1)

        model.resolveForegroundArrival(
            id: "approval-first",
            at: start.addingTimeInterval(2)
        )

        #expect(model.foregroundDispatch?.id == "question-second")
        #expect(model.foregroundDispatch?.phase == .reminding)
        #expect(model.foregroundQueuedCount == 0)
    }

    @MainActor
    @Test func unmatchedAgentDoesNotEnterFocusQueue() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        model.bindForegroundWorkspace(apps: [
            ForegroundWorkspaceApp(bundleIdentifier: "com.openai.codex", name: "Codex")
        ])

        model.receiveForegroundArrival(
            id: "claude-unbound",
            provider: .claude,
            eventKind: .question,
            title: "Claude 提问",
            taskTitle: nil
        )

        #expect(model.foregroundDispatch == nil)
        #expect(model.foregroundQueuedCount == 0)
    }

    @MainActor
    @Test func disabledAutoReturnLeavesAgentPageAndReleasesQueue() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        let start = Date(timeIntervalSince1970: 5_000)
        bindWorkspace(model)
        model.updateForegroundScheduling {
            $0.strategy = .immediate
            $0.returnsToActRealmWorkspace = false
            $0.acceptanceSeconds = 5
        }

        model.receiveForegroundArrival(
            id: "approval-no-return",
            provider: .codex,
            eventKind: .approval,
            title: "等待批准",
            taskTitle: nil,
            at: start
        )
        model.advanceForegroundDispatch(at: start.addingTimeInterval(2))
        #expect(model.foregroundDispatch?.phase == .awaitingWorkspace)
        model.advanceForegroundDispatch(at: start.addingTimeInterval(7))

        #expect(model.foregroundDispatch == nil)
        #expect(model.foregroundReturnNotes["approval-no-return"] == nil)
    }

    @Test func legacySchedulingSettingsGainAgentFocusDefaults() throws {
        let legacy = Data(#"{"isEnabled":true,"closesAfterAcceptance":true,"strategy":"remind","reminderSeconds":30,"returnsToActRealmWorkspace":false,"acceptanceSeconds":5,"codexRule":"actRealmWorkspace","claudeRule":"immediate"}"#.utf8)

        let settings = try JSONDecoder().decode(ForegroundSchedulingSettings.self, from: legacy)

        #expect(settings.isEnabled)
        #expect(settings.eventRules == AgentFocusEventRules())
        #expect(settings.strategy == .remind)
        #expect(settings.reminderSeconds == 30)
        #expect(settings.allowsStageManager)
        #expect(settings.stageManagerRestoreTiming == .onReturnToActRealm)
        #expect(!settings.returnsToActRealmWorkspace)
        #expect(settings.acceptanceSeconds == 5)
    }

    @MainActor
    @Test func workspaceBindingAndHUDSettingsPersist() {
        let suite = "ForegroundSchedulingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let first = AppModel(defaults: defaults, demo: false)

        first.bindForegroundWorkspace(
            apps: [
                ForegroundWorkspaceApp(bundleIdentifier: "com.example.agent", name: "Agent")
            ],
            displayID: 42,
            displayName: "Studio Display"
        )
        first.updateHUDSettings {
            $0.displaySeconds = 19
            $0.fields = [.event, .project]
            $0.displayMode = .selectedDisplay
            $0.selectedDisplayID = 84
            $0.selectedDisplayName = "Desk Display"
        }

        let restored = AppModel(defaults: defaults, demo: false)
        #expect(restored.foregroundScheduling.workspaceApps.map(\.bundleIdentifier) == ["com.example.agent"])
        #expect(restored.foregroundScheduling.workspaceDisplayID == 42)
        #expect(restored.foregroundScheduling.workspaceDisplayName == "Studio Display")
        #expect(restored.hudSettings.displaySeconds == 20)
        #expect(restored.hudSettings.fields == [.event, .project])
        #expect(restored.hudSettings.displayMode == .selectedDisplay)
        #expect(restored.hudSettings.selectedDisplayID == 84)
        #expect(restored.hudSettings.selectedDisplayName == "Desk Display")
    }

    @MainActor
    @Test func customThemeImageIsCopiedPersistedAndReset() throws {
        let suite = "ForegroundSchedulingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("ActRealmThemeTests-\(UUID().uuidString)", isDirectory: true)
        let themeDirectory = root.appendingPathComponent("Theme", isDirectory: true)
        let source = root.appendingPathComponent("source.png")
        defer {
            defaults.removePersistentDomain(forName: suite)
            try? FileManager.default.removeItem(at: root)
        }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let bytes = Data([0x89, 0x50, 0x4E, 0x47])
        try bytes.write(to: source)

        let first = AppModel(
            defaults: defaults,
            themeDirectory: themeDirectory,
            demo: false
        )
        try first.importThemeBackground(from: source, kind: .animatedImage)
        first.updateThemeSettings {
            $0.laneOpacity = 0.68
            $0.maintainsTransparencyWhenInactive = false
        }
        let copiedURL = try #require(first.themeBackgroundURL)
        #expect(copiedURL != source)
        #expect(FileManager.default.fileExists(atPath: copiedURL.path))
        #expect(try Data(contentsOf: copiedURL) == bytes)

        let restored = AppModel(
            defaults: defaults,
            themeDirectory: themeDirectory,
            demo: false
        )
        #expect(restored.themeBackgroundURL == copiedURL)
        #expect(restored.themeSettings.backgroundKind == .animatedImage)
        #expect(restored.themeSettings.laneOpacity == 0.68)
        #expect(!restored.themeSettings.maintainsTransparencyWhenInactive)
        restored.resetThemeBackground()
        #expect(restored.themeBackgroundURL == nil)
        #expect(restored.themeSettings.backgroundKind == .image)
        #expect(restored.themeSettings.laneOpacity == 0.68)
        #expect(!restored.themeSettings.maintainsTransparencyWhenInactive)
        #expect(!FileManager.default.fileExists(atPath: copiedURL.path))
    }

    @MainActor
    @Test func legacyThemeSettingsGainDynamicDefaultsAndOpacityIsClamped() throws {
        let legacy = Data(#"{"customBackgroundPath":null}"#.utf8)
        let decoded = try JSONDecoder().decode(AppThemeSettings.self, from: legacy)
        #expect(decoded.backgroundKind == .image)
        #expect(decoded.laneOpacity == 0.55)
        #expect(decoded.maintainsTransparencyWhenInactive)

        let oldTransparency = Data(#"{"laneTransparency":0.68}"#.utf8)
        let migrated = try JSONDecoder().decode(AppThemeSettings.self, from: oldTransparency)
        #expect(abs(migrated.laneOpacity - 0.32) < 0.0001)
        #expect(migrated.maintainsTransparencyWhenInactive)

        let nativeInactiveAppearance = Data(
            #"{"maintainsTransparencyWhenInactive":false}"#.utf8
        )
        let explicit = try JSONDecoder().decode(
            AppThemeSettings.self,
            from: nativeInactiveAppearance
        )
        #expect(!explicit.maintainsTransparencyWhenInactive)

        let model = AppModel(defaults: isolatedDefaults(), demo: false)
        model.updateThemeSettings { $0.laneOpacity = 2 }
        #expect(model.themeSettings.laneOpacity == 1)
        model.updateThemeSettings { $0.laneOpacity = -1 }
        #expect(model.themeSettings.laneOpacity == 0)
    }

    @MainActor
    private func bindWorkspace(_ model: AppModel) {
        model.bindForegroundWorkspace(apps: [
            ForegroundWorkspaceApp(bundleIdentifier: "com.openai.codex", name: "Codex"),
            ForegroundWorkspaceApp(bundleIdentifier: "com.anthropic.claude", name: "Claude"),
        ])
    }

    private func isolatedDefaults() -> UserDefaults {
        let suite = "ForegroundSchedulingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }
}
