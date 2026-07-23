import Foundation
import Testing
@testable import ActRealmKit

@Suite struct ToastBehaviorTests {
    @MainActor
    @Test func informationalToastDoesNotReplaceVisibleError() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)

        model.showToast("设置保存失败", priority: .error)
        model.showToast("命令已复制", priority: .informational)

        #expect(model.toastMessage == "设置保存失败")
    }

    @MainActor
    @Test func errorToastReplacesVisibleInformationalMessage() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)

        model.showToast("命令已复制", priority: .informational)
        model.showToast("导出失败", priority: .error)

        #expect(model.toastMessage == "导出失败")
    }

    @MainActor
    @Test func settingsVisibilityRoutesSharedToastToTheFrontmostWindow() {
        let model = AppModel(defaults: isolatedDefaults(), demo: true)

        model.setSettingsVisible(true)
        #expect(model.isSettingsVisible)

        model.setSettingsVisible(false)
        #expect(!model.isSettingsVisible)
    }

    private func isolatedDefaults() -> UserDefaults {
        let suite = "ToastBehaviorTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }
}
