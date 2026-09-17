import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct LocalizationTests {
    @Test func quotaWindowsUseSingularAndPluralEnglishWithoutChangingNames() {
        for unit in ["month", "week", "day", "hour", "minute"] {
            for count in ["1", "2"] {
                let message = RuntimeMessage(code: "quota.window.\(unit)s", args: ["count": count])
                #expect(AppLocalization.localizedRuntimeMessage(message, fallback: "unused", language: .english)
                    == "\(count) \(unit)\(count == "1" ? "" : "s")")
            }
        }
        let scoped = RuntimeMessage(code: "quota.window.scoped_weeks", args: ["count": "1", "name": "Fable"])
        #expect(AppLocalization.localizedRuntimeMessage(scoped, fallback: "unused", language: .english) == "Fable · 1 week")
        #expect(AppLocalization.localizedRuntimeMessage(scoped, fallback: "unused", language: .simplifiedChinese) == "Fable · 1 周")
        let unknown = RuntimeMessage(code: "provider.custom", args: ["count": "1"])
        #expect(AppLocalization.localizedRuntimeMessage(unknown, fallback: "Provider title", language: .english) == "Provider title")
    }

    @Test func freshAndInvalidPreferencesFollowTheSystemByDefault() {
        let suite = "ActRealmLanguageDefaults.\(UUID())"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        #expect(AppLocalization.selectedLanguage(defaults: defaults) == .system)
        defaults.set("invalid-locale", forKey: AppLanguage.preferenceKey)
        #expect(AppLocalization.selectedLanguage(defaults: defaults) == .system)
        defaults.set("en", forKey: AppLanguage.preferenceKey)
        #expect(AppLocalization.selectedLanguage(defaults: defaults) == .english)
        #expect(AppLanguage.resolvedIdentifier(selection: .english, preferredLanguages: ["zh-Hans-CN"]) == "en")
    }

    @Test func usageSourcesAndCoverageTranslateBeforeTheyAreCombined() {
        let sources: [String?] = [nil, "statusline", "claude_transcript_incremental", "codex_response_records",
            "codex_rollout_session_local", "codex_rollout_incremental", "codex_rollout_during_indexing"]
        for source in sources {
            for quality: String? in [nil, "official", "official_local", "derived", "partial", "suspect"] {
                let text = AppLocalization.localizedUsageDescription(source: source, quality: quality, language: .english)
                #expect(!text.unicodeScalars.contains { (0x3400...0x9FFF).contains($0.value) })
            }
        }
        #expect(AppLocalization.localizedUsageDescription(source: "codex_response_records", quality: "official_local", language: .english) == "Codex response records · Verified local records")
        #expect(AppLocalization.localizedProviderText("命令包含组合语法", language: .english) == "Compound shell command")
    }

    @Test func englishPickersAndEventDatesDoNotLeakSystemChinese() {
        for key in ["50K / 分钟", "100K / 分钟", "250K / 分钟", "500K / 分钟", "1M / 分钟", "万 / 亿"] {
            let text = AppLocalization.localized(key, language: .english)
            #expect(!text.unicodeScalars.contains { (0x3400...0x9FFF).contains($0.value) })
        }
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: 0)!
        calendar.locale = Locale(identifier: "zh-Hans-CN")
        let text = ZhFormat.timestamp(Date(timeIntervalSince1970: 1_789_600_000), calendar: calendar, language: .english)
        #expect(!text.unicodeScalars.contains { (0x3400...0x9FFF).contains($0.value) })
        #expect(text.contains("2026"))
    }

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
        ) == "Tasks: 5 · Waiting: 3")
        #expect(AppLocalization.localized("恢复状态", language: .english) == "Recovery status")
        #expect(AppLocalization.localized("settings.tab.agents", language: .english) == "Agents")
        #expect(AppLocalization.localized("Agent", language: .english) == "Agent")
        #expect(AppLocalization.localized(
            "历史数据部分可用",
            language: .simplifiedChinese
        ) == "历史数据部分可用")
        #expect(AppLocalization.localized(
            "历史数据部分可用",
            language: .english
        ) == "Partial history available")
    }





    @Test func runtimeSupervisorFailuresLocalizeDynamicArguments() {
        #expect(AppLocalization.localizedRuntimeSupervisorText(
            "无法停止旧 Runtime（PID 42）",
            language: .english
        ) == "Could not stop the previous Runtime (PID 42)")
        #expect(AppLocalization.localizedRuntimeSupervisorText(
            "runtime.lock 由未识别进程 PID 42 持有（/tmp/helper），为避免误杀未自动停止",
            language: .english
        ) == "runtime.lock is held by unrecognized process PID 42 (/tmp/helper), so ActRealm did not stop it")
        #expect(AppLocalization.localizedRuntimeSupervisorText(
            "cargo build --release -p actrealm failed",
            language: .english
        ) == "Could not build the local Runtime helper")
    }

    @Test func clientOwnedToastCopyUsesSelectedLanguage() {
        #expect(AppLocalization.localized(
            "Codex 启动命令已复制；运行后输入 /hooks",
            language: .english
        ) == "Codex launch command copied. Run it, then enter /hooks.")
        #expect(AppLocalization.formatted(
            "保存失败：%@",
            "Disk full",
            language: .english
        ) == "Could not save: Disk full")
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
