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
        bindWorkspace(model)

        model.receiveForegroundArrival(
            id: "approval-1",
            provider: .codex,
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
        #expect(model.foregroundReturnNotes["approval-1"] == "未检测到进入调度工作桌面，已返回")

        model.advanceForegroundDispatch(at: start.addingTimeInterval(25))
        #expect(model.foregroundDispatch == nil)
    }

    @MainActor
    @Test func providerOverrideAndManualReceiveAreApplied() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)
        bindWorkspace(model)
        #expect(model.effectiveForegroundStrategy(for: .claude) == .immediate)
        #expect(model.effectiveForegroundStrategy(for: .codex) == .remind)

        model.receiveForegroundArrival(
            id: "question-1",
            provider: .claude,
            title: "Claude 发出一个待回答问题",
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
        #expect(!model.foregroundScheduling.isEnabled)
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
        #expect(model.toastMessage?.contains("绑定协作桌面") == true)
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
        }

        let restored = AppModel(defaults: defaults, demo: false)
        #expect(restored.foregroundScheduling.workspaceApps.map(\.bundleIdentifier) == ["com.example.agent"])
        #expect(restored.foregroundScheduling.workspaceDisplayID == 42)
        #expect(restored.foregroundScheduling.workspaceDisplayName == "Studio Display")
        #expect(restored.hudSettings.displaySeconds == 20)
        #expect(restored.hudSettings.fields == [.event, .project])
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
        first.updateThemeSettings { $0.laneOpacity = 0.68 }
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
        restored.resetThemeBackground()
        #expect(restored.themeBackgroundURL == nil)
        #expect(restored.themeSettings.backgroundKind == .image)
        #expect(restored.themeSettings.laneOpacity == 0.68)
        #expect(!FileManager.default.fileExists(atPath: copiedURL.path))
    }

    @MainActor
    @Test func legacyThemeSettingsGainDynamicDefaultsAndOpacityIsClamped() throws {
        let legacy = Data(#"{"customBackgroundPath":null}"#.utf8)
        let decoded = try JSONDecoder().decode(AppThemeSettings.self, from: legacy)
        #expect(decoded.backgroundKind == .image)
        #expect(decoded.laneOpacity == 0.55)

        let oldTransparency = Data(#"{"laneTransparency":0.68}"#.utf8)
        let migrated = try JSONDecoder().decode(AppThemeSettings.self, from: oldTransparency)
        #expect(abs(migrated.laneOpacity - 0.32) < 0.0001)

        let model = AppModel(defaults: isolatedDefaults(), demo: false)
        model.updateThemeSettings { $0.laneOpacity = 2 }
        #expect(model.themeSettings.laneOpacity == 1)
        model.updateThemeSettings { $0.laneOpacity = -1 }
        #expect(model.themeSettings.laneOpacity == 0)
    }

    @MainActor
    private func bindWorkspace(_ model: AppModel) {
        model.bindForegroundWorkspace(apps: [
            ForegroundWorkspaceApp(bundleIdentifier: "com.example.agent", name: "Agent")
        ])
    }

    private func isolatedDefaults() -> UserDefaults {
        let suite = "ForegroundSchedulingTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }
}
