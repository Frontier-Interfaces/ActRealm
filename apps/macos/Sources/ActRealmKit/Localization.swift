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
        "cost": ("估算 API 价格", "估算值，不是订阅账单"),
        "turnTokens": ("本轮 Token", "最近一轮 Token"),
        "inputOutputTokens": ("输入 / 输出 Token", "输入与输出拆分"),
        "cacheTokens": ("缓存读取 / 写入 Token", "缓存用量拆分"),
        "reasoningTokens": ("推理 Token", "Provider 推理用量"),
        "tool": ("当前工具", nil),
        "permissionMode": ("权限模式", nil),
        "subagents": ("运行中的子 Agent", nil),
        "environment": ("运行环境", nil),
        "recovery": ("恢复状态", nil),
        "control": ("托管能力", nil),
        "jump": ("打开应用", nil),
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

    private static func localizationBundle(identifier: String) -> Bundle? {
        for container in [Bundle.module, Bundle.main] {
            guard let resourcesURL = container.resourceURL else { continue }
            let localizationURL = resourcesURL
                .appendingPathComponent(identifier, isDirectory: true)
                .appendingPathExtension("lproj")
            if let bundle = Bundle(url: localizationURL) {
                return bundle
            }
        }
        return nil
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
