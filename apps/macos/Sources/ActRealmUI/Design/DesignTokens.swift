import AppKit
import ActRealmKit
import SwiftUI

extension Color {
    /// Dynamic color that follows the window's light/dark appearance.
    init(light: NSColor, dark: NSColor) {
        self.init(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            return isDark ? dark : light
        })
    }

    init(lightWhite alpha: CGFloat, darkWhite darkAlpha: CGFloat) {
        self.init(
            light: NSColor(white: 0, alpha: alpha),
            dark: NSColor(white: 1, alpha: darkAlpha)
        )
    }
}

extension NSColor {
    static func rgba(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat) -> NSColor {
        NSColor(srgbRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
    }
}

/// Design tokens from the Lanes+ handoff (light values authoritative,
/// dark values from the HTML prototype's dark screens).
enum DT {
    // MARK: Text

    static let textPrimary = Color(lightWhite: 0.88, darkWhite: 0.92)
    static let textStrong = Color(lightWhite: 0.9, darkWhite: 0.95)
    static let textSecondary = Color(lightWhite: 0.48, darkWhite: 0.5)
    static let textWeak = Color(lightWhite: 0.4, darkWhite: 0.45)
    static let textFaint = Color(lightWhite: 0.35, darkWhite: 0.38)

    // MARK: Amber (waiting / needs you)

    static let amberBg = Color(
        light: .rgba(255, 159, 10, 0.16), dark: .rgba(255, 159, 10, 0.13))
    static let amberStroke = Color(
        light: .rgba(214, 126, 20, 0.38), dark: .rgba(255, 159, 10, 0.32))
    static let amberText = Color(
        light: .rgba(154, 91, 0, 1), dark: .rgba(255, 179, 64, 1))
    static let amberTextSoft = Color(
        light: .rgba(160, 90, 0, 1), dark: .rgba(255, 217, 160, 1))
    static let amberDot = Color(light: .rgba(255, 159, 10, 1), dark: .rgba(255, 159, 10, 1))

    // MARK: Red (high risk / tight quota)

    static let redBg = Color(light: .rgba(255, 59, 48, 0.12), dark: .rgba(255, 69, 58, 0.16))
    static let redStroke = Color(light: .rgba(255, 59, 48, 0.38), dark: .rgba(255, 69, 58, 0.4))
    static let redText = Color(light: .rgba(197, 50, 41, 1), dark: .rgba(255, 138, 128, 1))
    static let redRing = Color(light: .rgba(255, 59, 48, 1), dark: .rgba(255, 69, 58, 1))

    // MARK: Green (done / online / undo)

    static let greenBg = Color(light: .rgba(52, 199, 89, 0.13), dark: .rgba(48, 209, 88, 0.15))
    static let greenStroke = Color(light: .rgba(52, 199, 89, 0.35), dark: .rgba(48, 209, 88, 0.3))
    static let greenText = Color(light: .rgba(30, 158, 70, 1), dark: .rgba(48, 209, 88, 1))
    static let greenDot = Color(light: .rgba(52, 199, 89, 1), dark: .rgba(48, 209, 88, 1))

    // MARK: Blue (running / primary)

    static let blue = Color(light: .rgba(0, 122, 255, 1), dark: .rgba(10, 132, 255, 1))
    static let blueText = Color(light: .rgba(0, 98, 204, 1), dark: .rgba(100, 170, 255, 1))
    static let blueBg = Color(light: .rgba(0, 122, 255, 0.1), dark: .rgba(10, 132, 255, 0.16))
    static let blueBadgeStroke = Color(light: .rgba(0, 122, 255, 0.3), dark: .rgba(10, 132, 255, 0.35))
    static let primaryGradient = LinearGradient(
        colors: [
            Color(light: .rgba(47, 143, 255, 1), dark: .rgba(63, 155, 255, 1)),
            Color(light: .rgba(0, 122, 255, 1), dark: .rgba(10, 132, 255, 1)),
        ],
        startPoint: .top, endPoint: .bottom
    )

    // MARK: Neutral chips

    static let neutralChipBg = Color(lightWhite: 0.05, darkWhite: 0.08)
    static let neutralChipStroke = Color(lightWhite: 0.07, darkWhite: 0.1)
    static let neutralBadgeBg = Color(lightWhite: 0.06, darkWhite: 0.1)
    static let neutralBadgeStroke = Color(lightWhite: 0.1, darkWhite: 0.14)

    // MARK: Provider identity

    static let logoTint = Color(light: .rgba(59, 110, 240, 1), dark: .rgba(122, 162, 255, 1))

    static func providerText(_ kind: ProviderKind) -> Color {
        switch kind {
        case .claude: Color(light: .rgba(192, 95, 46, 1), dark: .rgba(232, 146, 92, 1))
        case .codex: Color(light: .rgba(18, 131, 106, 1), dark: .rgba(67, 201, 162, 1))
        case .gemini: Color(light: .rgba(59, 110, 240, 1), dark: .rgba(122, 162, 255, 1))
        }
    }

    static func providerBg(_ kind: ProviderKind) -> Color {
        switch kind {
        case .claude: Color(light: .rgba(217, 119, 72, 0.16), dark: .rgba(232, 146, 92, 0.2))
        case .codex: Color(light: .rgba(23, 160, 122, 0.13), dark: .rgba(67, 201, 162, 0.18))
        case .gemini: Color(light: .rgba(59, 110, 240, 0.12), dark: .rgba(122, 162, 255, 0.18))
        }
    }

    static func providerStroke(_ kind: ProviderKind) -> Color {
        switch kind {
        case .claude: Color(light: .rgba(217, 119, 72, 0.4), dark: .rgba(232, 146, 92, 0.38))
        case .codex: Color(light: .rgba(23, 160, 122, 0.38), dark: .rgba(67, 201, 162, 0.35))
        case .gemini: Color(light: .rgba(59, 110, 240, 0.32), dark: .rgba(122, 162, 255, 0.35))
        }
    }

    // MARK: Surfaces (sheets laid on the glassy window)

    static let cardStrong = Color(
        light: NSColor(white: 1, alpha: 0.78), dark: .rgba(30, 32, 42, 0.72))
    /// Exact fill used by the three primary lanes. Unlike applying opacity to
    /// `cardStrong`, this makes the slider endpoints literal: 0 is clear and
    /// 1 is fully opaque in both appearances.
    static func mainLaneFill(opacity: Double) -> Color {
        let alpha = CGFloat(min(1, max(0, opacity)))
        return Color(
            light: NSColor(white: 1, alpha: alpha),
            dark: .rgba(30, 32, 42, alpha)
        )
    }
    static let cardMedium = Color(
        light: NSColor(white: 1, alpha: 0.62), dark: .rgba(30, 32, 42, 0.55))
    static let cardSoft = Color(
        light: NSColor(white: 1, alpha: 0.5), dark: .rgba(30, 32, 42, 0.42))
    static let cardFaint = Color(
        light: NSColor(white: 1, alpha: 0.42), dark: .rgba(30, 32, 42, 0.34))
    static let laneBg = Color(
        light: NSColor(white: 1, alpha: 0.5), dark: .rgba(24, 26, 34, 0.52))
    static let hairline = Color(
        light: NSColor(white: 1, alpha: 0.75), dark: NSColor(white: 1, alpha: 0.12))
    static let hairlineSoft = Color(
        light: NSColor(white: 1, alpha: 0.6), dark: NSColor(white: 1, alpha: 0.09))
    static let innerHighlight = Color(
        light: NSColor(white: 1, alpha: 0.9), dark: NSColor(white: 1, alpha: 0.16))
    static let separator = Color(lightWhite: 0.07, darkWhite: 0.1)
    static let commandBoxBg = Color(
        light: NSColor(white: 1, alpha: 0.7), dark: .rgba(0, 0, 0, 0.32))
    static let commandBoxStroke = Color(lightWhite: 0.08, darkWhite: 0.08)
    static let buttonSecondaryBg = Color(
        light: NSColor(white: 1, alpha: 0.75), dark: NSColor(white: 1, alpha: 0.1))
    static let buttonSecondaryStroke = Color(lightWhite: 0.12, darkWhite: 0.16)
    static let buttonTertiaryBg = Color(
        light: NSColor(white: 1, alpha: 0.5), dark: NSColor(white: 1, alpha: 0.06))
    static let buttonTertiaryStroke = Color(lightWhite: 0.09, darkWhite: 0.12)
    static let ringTrack = Color(lightWhite: 0.1, darkWhite: 0.14)
    static let progressTrack = Color(lightWhite: 0.1, darkWhite: 0.14)
    static let cardShadow = Color(
        light: .rgba(40, 60, 120, 0.18), dark: .rgba(0, 0, 0, 0.5))
    static let softShadow = Color(
        light: .rgba(40, 60, 120, 0.07), dark: .rgba(0, 0, 0, 0.3))

    // MARK: Radii

    static let radiusWindow: CGFloat = 22
    static let radiusLane: CGFloat = 18
    static let radiusCardLarge: CGFloat = 17
    static let radiusCard: CGFloat = 13
    static let radiusCommand: CGFloat = 10

    // MARK: Type

    static func sectionTitle() -> Font { .system(size: 12, weight: .heavy) }
    static func cardTitle(_ size: CGFloat = 12.5) -> Font { .system(size: size, weight: .bold) }
    static func body(_ size: CGFloat = 11) -> Font { .system(size: size) }
    static func micro(_ size: CGFloat = 9.5, weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight)
    }
    static func mono(_ size: CGFloat = 12, weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }
}
