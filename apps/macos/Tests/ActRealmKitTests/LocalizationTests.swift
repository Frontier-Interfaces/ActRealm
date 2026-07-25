import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct LocalizationTests {
    @Test func systemLanguageResolvesChineseOrEnglish() {
        #expect(AppLanguage.resolvedIdentifier(
            selection: .system,
            preferredLanguages: ["zh-Hans-CN"]
        ) == "zh-Hans")
        #expect(AppLanguage.resolvedIdentifier(
            selection: .system,
            preferredLanguages: ["en-US"]
        ) == "en")
        #expect(AppLanguage.resolvedIdentifier(
            selection: .system,
            preferredLanguages: ["ja-JP"]
        ) == "en")
        #expect(AppLanguage.resolvedIdentifier(
            selection: .simplifiedChinese,
            preferredLanguages: ["en-US"]
        ) == "zh-Hans")
    }

    @Test func resourcesTranslateFixedAndFormattedCopy() {
        #expect(AppLocalization.localized("通用", language: .simplifiedChinese) == "通用")
        #expect(AppLocalization.localized("通用", language: .english) == "General")
        #expect(AppLocalization.localized("等待批准", language: .english) == "Waiting for approval")
        #expect(AppLocalization.formatted(
            "%lld 项待处理",
            Int64(2),
            language: .english
        ) == "2 waiting")
        #expect(AppLocalization.formatted(
            "%lld 个任务 · %lld 等待",
            Int64(5),
            Int64(3),
            language: .english
        ) == "5 tasks · 3 waiting")
    }

    @Test func compactDurationsSupportBothLanguages() {
        #expect(ZhFormat.waitDuration(62, language: .simplifiedChinese) == "1 分 02 秒")
        #expect(ZhFormat.waitDuration(62, language: .english) == "1 min 02 sec")
        #expect(ZhFormat.relativeAgo(7_200, language: .english) == "2 hours ago")
    }

    @Test func knownGeneratedProviderPhrasesTranslateWithoutTouchingSourceContent() {
        #expect(AppLocalization.localizedProviderText(
            "正在运行 Bash",
            language: .english
        ) == "Running Bash")
        #expect(AppLocalization.localizedProviderText(
            "730 小时",
            language: .english
        ) == "730 hours")
        #expect(AppLocalization.localizedProviderText(
            "用户写下的任意内容",
            language: .english
        ) == "用户写下的任意内容")
    }

    @Test func runtimeMessagesUseStableCodesAndArgumentsAcrossLanguages() {
        let message = RuntimeMessage(
            code: "session.activity.tool_running",
            args: ["tool": "Bash"]
        )
        #expect(AppLocalization.localizedRuntimeMessage(
            message,
            fallback: "Running Bash",
            language: .english
        ) == "Running Bash")
        #expect(AppLocalization.localizedRuntimeMessage(
            message,
            fallback: "Running Bash",
            language: .simplifiedChinese
        ) == "正在运行 Bash")
        #expect(AppLocalization.localizedRuntimeMessage(
            RuntimeMessage(code: "future.message"),
            fallback: "Future fallback",
            language: .simplifiedChinese
        ) == "Future fallback")
    }

    @Test func apiErrorsUseClientOwnedWording() {
        #expect(AppLocalization.localizedAPIError(
            "QUESTION_EXPIRED",
            language: .english
        ) == "This question has expired and cannot be submitted")
        #expect(AppLocalization.localizedAPIError(
            "QUESTION_EXPIRED",
            language: .simplifiedChinese
        ) == "这个问题已经过期，不能再提交")
        #expect(AppLocalization.localizedAPIError(
            "HTTP_503",
            language: .english
        ) == "Local request failed (HTTP 503)")
        #expect(AppLocalization.localizedAPIError(
            "The connection was reset by peer",
            language: .simplifiedChinese
        ) == "请求失败，请重试")
    }

    @Test @MainActor func appLanguageSelectionPersistsLocally() {
        let suite = "ActRealmLocalizationTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let first = AppModel(defaults: defaults, demo: true)
        #expect(first.appLanguage == .system)
        first.setAppLanguage(.english)
        #expect(first.appLanguage == .english)

        let restored = AppModel(defaults: defaults, demo: true)
        #expect(restored.appLanguage == .english)
        #expect(restored.interfaceLocale.identifier.lowercased().hasPrefix("en"))
    }

    @Test @MainActor func switchingLanguageClearsRenderedTransientCopy() {
        let defaults = UserDefaults(suiteName: "ActRealmLocalizationTests.\(UUID().uuidString)")!
        let model = AppModel(defaults: defaults, demo: true)
        model.showToast("设置已保存在本机")

        model.setAppLanguage(.english)

        #expect(model.toastMessage == nil)
    }

    @Test func menuBarPresentationCanRenderEnglishWithoutChangingCoreState() {
        let presentation = MenuBarLanePresentation(
            provider: .codex,
            tasks: [],
            now: .now,
            language: .english
        )
        #expect(presentation.subtitle == "Codex · No active tasks")
        #expect(presentation.trailing == "No activity")
    }
}
