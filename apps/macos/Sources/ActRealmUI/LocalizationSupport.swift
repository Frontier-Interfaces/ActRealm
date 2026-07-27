import ActRealmKit
import Foundation

@inline(__always)
func localized(_ key: String, locale: Locale) -> String {
    AppLocalization.localized(key, locale: locale)
}

@inline(__always)
func localizedFormat(
    _ key: String,
    locale: Locale,
    _ arguments: CVarArg...
) -> String {
    let format = AppLocalization.localized(key, locale: locale)
    return String(format: format, locale: locale, arguments: arguments)
}
