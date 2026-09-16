import Foundation

public enum AppLanguage: String, CaseIterable, Codable, Sendable {
    case system
    case simplifiedChinese = "zh-Hans"
    case english = "en"

    public static let preferenceKey = "actrealm.appLanguage"

    public static func resolvedIdentifier(
        selection: AppLanguage,
        preferredLanguages: [String] = Locale.preferredLanguages
    ) -> String {
        switch selection {
        case .simplifiedChinese:
            return "zh-Hans"
        case .english:
            return "en"
        case .system:
            let preferred = preferredLanguages.first?.lowercased() ?? "en"
            return preferred.hasPrefix("zh") ? "zh-Hans" : "en"
        }
    }

    public var locale: Locale {
        Locale(identifier: Self.resolvedIdentifier(selection: self))
    }

    public static func resolvedIdentifier(for locale: Locale) -> String {
        locale.identifier.lowercased().hasPrefix("zh") ? "zh-Hans" : "en"
    }
}

public enum AppLocalization {
    private static let displayFieldKeys: [String: (label: String, description: String?)] = [
        "task": ("任务标题与摘要", "主标题；不同的任务摘要显示在下一行"),
        "activity": ("实时状态", "标题栏右侧的运行阶段与耗时"),
        "project": ("项目", "副标题中的项目名称"),
        "model": ("模型", "副标题中的模型名称"),
        "plan": ("计划进度", "完成步数与进度条"),
        "sessionTokens": ("会话累计 Token", "折叠卡用量胶囊"),
        "context": ("上下文占用", "当前上下文百分比"),
        "cost": ("API 等价值", "理论 API 计价，不是订阅账单或实际支出"),
        "turnTokens": ("本轮 Token", "最近一轮 Token"),
        "inputOutputTokens": ("输入 / 输出 Token", "输入与输出拆分"),
        "cacheTokens": ("缓存读取 / 写入 Token", "缓存用量拆分"),
        "reasoningTokens": ("推理 Token", "Provider 推理用量"),
        "tool": ("当前动作", "语义类别与 Provider 工具名"),
        "currentTarget": ("当前文件 / 目标", "仅使用 Provider 明确 path 字段的 basename"),
        "permissionMode": ("权限模式", nil),
        "subagents": ("运行中的子 Agent", nil),
        "environment": ("运行环境", nil),
        "recovery": ("恢复状态", nil),
        "control": ("托管能力", nil),
        "jump": ("打开应用", nil),
        "taskFlow": ("任务流程", "展开任务后显示当前 Turn 的结构化计划步骤"),
        "workflow": ("最近活动", "展开任务后显示当前 Turn 的重要工具活动"),
        "titleSource": ("标题来源", nil),
        "sessionId": ("ActRealm Session ID", nil),
        "providerSessionId": ("Provider Session ID", nil),
        "providerTurnId": ("Provider Turn ID", nil),
        "lastEventAt": ("最后事件时间", nil),
    ]

    public static func selectedLanguage(defaults: UserDefaults = .standard) -> AppLanguage {
        defaults.string(forKey: AppLanguage.preferenceKey)
            .flatMap(AppLanguage.init(rawValue:)) ?? .system
    }

    public static func localized(
        _ key: String,
        language: AppLanguage? = nil,
        defaults: UserDefaults = .standard
    ) -> String {
        let selected = language ?? selectedLanguage(defaults: defaults)
        let identifier = AppLanguage.resolvedIdentifier(selection: selected)
        guard let bundle = localizationBundle(identifier: identifier) else {
            return key
        }
        return bundle.localizedString(forKey: key, value: key, table: nil)
    }

    public static func localized(_ key: String, locale: Locale) -> String {
        let identifier = AppLanguage.resolvedIdentifier(for: locale)
        guard let bundle = localizationBundle(identifier: identifier) else {
            return key
        }
        return bundle.localizedString(forKey: key, value: key, table: nil)
    }

    private static let mainLocalizationBundles = localizationBundles(
        in: Bundle.main
    )

    private static let moduleLocalizationBundles = localizationBundles(
        in: Bundle.module
    )

    private static func localizationBundle(identifier: String) -> Bundle? {
        // Packaged apps copy the language tables into Bundle.main. Prefer that
        // path so window restoration never has to resolve SwiftPM's Bundle.module,
        // whose generated fallback contains an absolute build-machine path.
        // The right-hand side of ?? remains lazy, preserving package-test and
        // development support when the executable has no embedded tables.
        mainLocalizationBundles[identifier]
            ?? moduleLocalizationBundles[identifier]
    }

    private static func localizationBundles(in container: Bundle) -> [String: Bundle] {
        guard let resourcesURL = container.resourceURL else { return [:] }
        return Dictionary(uniqueKeysWithValues: ["zh-Hans", "en"].compactMap {
            identifier -> (String, Bundle)? in
            let localizationURL = resourcesURL
                .appendingPathComponent(identifier, isDirectory: true)
                .appendingPathExtension("lproj")
            guard let bundle = Bundle(url: localizationURL) else { return nil }
            return (identifier, bundle)
        })
    }

    public static func formatted(
        _ key: String,
        _ arguments: CVarArg...,
        language: AppLanguage? = nil,
        defaults: UserDefaults = .standard
    ) -> String {
        let selected = language ?? selectedLanguage(defaults: defaults)
        let format = localized(key, language: selected, defaults: defaults)
        return String(
            format: format,
            locale: selected.locale,
            arguments: arguments
        )
    }

    public static func formatted(
        _ key: String,
        _ arguments: CVarArg...,
        locale: Locale
    ) -> String {
        let format = localized(key, locale: locale)
        return String(format: format, locale: locale, arguments: arguments)
    }

    public static func localizedRuntimeMessage(
        _ message: RuntimeMessage?,
        fallback: String,
        language: AppLanguage
    ) -> String {
        guard let message else {
            return localizedProviderText(fallback, language: language)
        }
        let template = localized(message.code, language: language)
        guard template != message.code else {
            return localizedProviderText(fallback, language: language)
        }
        return message.args.reduce(template) { result, pair in
            result.replacingOccurrences(of: "{\(pair.key)}", with: pair.value)
        }
    }

    public static func localizedRuntimeCode(
        _ code: String?,
        fallback: String,
        language: AppLanguage
    ) -> String {
        localizedRuntimeMessage(
            code.map { RuntimeMessage(code: $0) },
            fallback: fallback,
            language: language
        )
    }

    /// Localize the components before combining them; translating a fully
    /// assembled source/quality string leaves Chinese fragments in English UI.
    public static func localizedUsageDescription(source: String?, quality: String?, language: AppLanguage) -> String {
        let sourceKey = switch source {
        case "statusline": "StatusLine"
        case "claude_transcript": "Claude transcript"
        case "claude_transcript_incremental": "Claude transcript（增量）"
        case "codex_rollout": "Codex 本机 rollout"
        case "codex_response_records": "Codex 本机响应账本"
        case "codex_rollout_session_local": "Codex 本机会话记录"
        case "codex_rollout_incremental": "Codex 本机 rollout（增量）"
        case "codex_rollout_during_indexing": "Codex 本机记录（历史索引中）"
        default: "Provider 未提供用量数据"
        }
        let qualityKey = switch quality {
        case "official": "官方"
        case "official_local": "已验证本机记录"
        case "derived": "完整派生"
        case "partial": "部分覆盖"
        case "suspect": "数据可疑"
        default: "完整性未知"
        }
        return localized(sourceKey, language: language) + " · " + localized(qualityKey, language: language)
    }

    public static func localizedAPIError(
        _ code: String?,
        language: AppLanguage
    ) -> String {
        guard let code, !code.isEmpty else {
            return localized("未知错误", language: language)
        }
        let direct = localized(code, language: language)
        if direct != code {
            return direct
        }
        if let status = code.split(separator: "_").last,
           code.hasPrefix("HTTP_"),
           Int(status) != nil
        {
            return formatted(
                "本机请求失败（HTTP %@）",
                String(status),
                language: language
            )
        }
        if code.allSatisfy({ $0.isUppercase || $0.isNumber || $0 == "_" }) {
            return formatted("请求失败（%@）", code, language: language)
        }
        return localized("请求失败，请重试", language: language)
    }

    /// Localizes ActRealm-owned Runtime supervisor diagnostics while leaving
    /// paths, PIDs and provider/system output verbatim. The supervisor stores
    /// operational facts independently from the currently selected UI locale.
    public static func localizedRuntimeSupervisorText(
        _ message: String,
        language: AppLanguage
    ) -> String {
        guard AppLanguage.resolvedIdentifier(selection: language) == AppLanguage.english.rawValue
        else { return message }

        let direct = localized(message, language: language)
        if direct != message { return direct }

        if let pid = value(in: message, prefix: "无法停止旧 Runtime（PID ", suffix: "）") {
            return formatted("runtime.error.stop_old", pid, language: language)
        }
        if let pid = value(in: message, prefix: "无法安全替换遗留 Runtime（PID ", suffix: "）") {
            return formatted("runtime.error.replace_abandoned", pid, language: language)
        }
        if message.hasPrefix("failed to launch actrealm: ") {
            return formatted(
                "runtime.error.launch",
                String(message.dropFirst("failed to launch actrealm: ".count)),
                language: language
            )
        }
        if let status = value(in: message, prefix: "actrealm 退出（状态码 ", suffix: "）") {
            return formatted("runtime.error.exit_status", status, language: language)
        }
        if message.hasPrefix("runtime.lock 由未识别进程 PID "),
           let held = message.range(of: " 持有（"),
           let suffix = message.range(of: "），为避免误杀未自动")
        {
            let pidStart = message.index(message.startIndex, offsetBy: "runtime.lock 由未识别进程 PID ".count)
            let pid = String(message[pidStart..<held.lowerBound])
            let path = String(message[held.upperBound..<suffix.lowerBound])
            let action = String(message[suffix.upperBound...])
            let key = action == "停止"
                ? "runtime.error.unrecognized_lock_stop"
                : "runtime.error.unrecognized_lock_takeover"
            return formatted(key, pid, path, language: language)
        }
        if message.hasSuffix("；自动恢复连续失败 5 次，已停止重试") {
            let base = String(message.dropLast("；自动恢复连续失败 5 次，已停止重试".count))
            return formatted(
                "runtime.error.restart_exhausted",
                localizedRuntimeSupervisorText(base, language: language),
                language: language
            )
        }
        if let marker = message.range(of: "；将在 "), message.hasSuffix(" 后自动重启") {
            let failure = String(message[..<marker.lowerBound])
            let delay = String(message[marker.upperBound...].dropLast(" 后自动重启".count))
            return formatted(
                "runtime.error.restart_scheduled",
                localizedRuntimeSupervisorText(failure, language: language),
                localizedProviderText(delay, language: language),
                language: language
            )
        }
        return message
    }

    private static func value(in text: String, prefix: String, suffix: String) -> String? {
        guard text.hasPrefix(prefix), text.hasSuffix(suffix) else { return nil }
        return String(text.dropFirst(prefix.count).dropLast(suffix.count))
    }

    public static func localizedDisplayFieldLabel(
        id: String,
        fallback _: String,
        language: AppLanguage
    ) -> String {
        guard let key = displayFieldKeys[id]?.label else { return id }
        return localized(key, language: language)
    }

    public static func localizedDisplayFieldDescription(
        id: String,
        fallback _: String?,
        language: AppLanguage
    ) -> String? {
        guard let key = displayFieldKeys[id]?.description else { return nil }
        return localized(key, language: language)
    }

    /// Compatibility translator for legacy Runtime-generated status phrases.
    /// Never apply this heuristic to user- or Provider-authored task titles.
    public static func localizedProviderText(
        _ text: String,
        language: AppLanguage
    ) -> String {
        let direct = localized(text, language: language)
        guard direct == text,
              AppLanguage.resolvedIdentifier(selection: language) == AppLanguage.english.rawValue
        else {
            return direct
        }

        let prefixMappings: [(String, String)] = [
            ("正在运行 ", "Running "),
            ("允许 ", "Allow "),
            ("将允许", "Will allow "),
            ("将拒绝", "Will deny "),
            ("将交回", "Will return "),
            ("已允许", "Allowed "),
            ("已拒绝", "Denied "),
            ("已交回", "Returned "),
        ]
        for (source, replacement) in prefixMappings where text.hasPrefix(source) {
            var remainder = String(text.dropFirst(source.count))
            remainder = remainder.replacingOccurrences(of: " 运行 ", with: " to run ")
            if remainder.hasSuffix(" 的请求") {
                remainder = String(remainder.dropLast(" 的请求".count)) + "'s request"
            }
            return replacement + remainder
                .replacingOccurrences(of: "？", with: "?")
        }

        if text.hasPrefix("等待批准（"), text.hasSuffix("）") {
            let detail = text
                .dropFirst("等待批准（".count)
                .dropLast()
            return "Waiting for approval (\(detail))"
        }

        let components = text.split(separator: " ", omittingEmptySubsequences: true)
        if components.count == 2, let value = Int(components[0]) {
            switch components[1] {
            case "个月": return "\(value) \(value == 1 ? "month" : "months")"
            case "周": return "\(value) \(value == 1 ? "week" : "weeks")"
            case "天": return "\(value) \(value == 1 ? "day" : "days")"
            case "小时": return "\(value) \(value == 1 ? "hour" : "hours")"
            case "分钟": return "\(value) \(value == 1 ? "minute" : "minutes")"
            default: break
            }
        }
        return text
    }
}
