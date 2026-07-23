import AppKit
import ActRealmKit
import SwiftUI

// MARK: - Logo

@MainActor
private enum ActRealmBrandAsset {
    static let image: NSImage = {
        if let packaged = Bundle.main.url(
            forResource: "ActRealmIcon",
            withExtension: "png"
        ), let image = NSImage(contentsOf: packaged) {
            return image
        }

        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<4 { root.deleteLastPathComponent() }
        let developmentAsset = root
            .appendingPathComponent("Resources", isDirectory: true)
            .appendingPathComponent("ActRealmIcon.png")
        return NSImage(contentsOf: developmentAsset)
            ?? NSApplication.shared.applicationIconImage
    }()
}

/// ActRealm's app icon, used anywhere the product itself is represented.
struct LogoMark: View {
    var size: CGFloat = 18

    var body: some View {
        Image(nsImage: ActRealmBrandAsset.image)
            .resizable()
            .interpolation(.high)
            .scaledToFit()
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

/// Larger variant for surfaces that need a standalone app tile.
struct AppMark: View {
    var size: CGFloat = 27

    var body: some View {
        LogoMark(size: size)
    }
}

// MARK: - Provider avatar

struct ProviderAvatar: View {
    let kind: ProviderKind
    var size: CGFloat = 24

    var body: some View {
        Group {
            if let icon = providerIcon {
                Image(nsImage: icon)
                    .resizable()
                    .interpolation(.high)
            } else {
                Image(systemName: "app.dashed")
                    .resizable()
                    .scaledToFit()
                    .padding(size * 0.2)
                    .foregroundStyle(DT.providerText(kind))
                    .background(DT.providerBg(kind))
            }
        }
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: size * 0.23, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: size * 0.23, style: .continuous)
                    .strokeBorder(DT.providerStroke(kind).opacity(0.65), lineWidth: 0.75)
            )
    }

    private var providerIcon: NSImage? {
        let bundleIdentifiers: [String]
        let applicationPaths: [String]
        let assetName: String
        switch kind {
        case .claude:
            bundleIdentifiers = ["com.anthropic.claudefordesktop"]
            applicationPaths = ["/Applications/Claude.app"]
            assetName = "claude.png"
        case .codex:
            bundleIdentifiers = ["com.openai.codex", "com.openai.chat"]
            applicationPaths = ["/Applications/Codex.app"]
            assetName = "codex.png"
        case .gemini:
            bundleIdentifiers = []
            applicationPaths = []
            assetName = ""
        case .custom:
            bundleIdentifiers = []
            applicationPaths = []
            assetName = ""
        }

        for identifier in bundleIdentifiers {
            if let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: identifier) {
                return NSWorkspace.shared.icon(forFile: url.path)
            }
        }
        for path in applicationPaths where FileManager.default.fileExists(atPath: path) {
            return NSWorkspace.shared.icon(forFile: path)
        }
        guard !assetName.isEmpty else { return nil }
        if let packaged = Bundle.main.resourceURL?
            .appendingPathComponent("ProviderIcons", isDirectory: true)
            .appendingPathComponent(assetName),
           let image = NSImage(contentsOf: packaged)
        {
            return image
        }
        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<6 { root.deleteLastPathComponent() }
        return NSImage(contentsOf: root.appendingPathComponent("web/assets/\(assetName)"))
    }
}

// MARK: - Chips & badges

struct Chip: View {
    enum Tone { case amber, red, green, blue, neutral, provider(ProviderKind) }

    let text: String
    var tone: Tone = .neutral
    var fontSize: CGFloat = 9.5

    var body: some View {
        Text(text)
            .font(.system(size: fontSize, weight: .bold))
            .foregroundStyle(foreground)
            .padding(.horizontal, 7)
            .padding(.vertical, 1.5)
            .background(Capsule().fill(background))
            .overlay(Capsule().strokeBorder(stroke, lineWidth: 1))
            .fixedSize()
    }

    private var foreground: Color {
        switch tone {
        case .amber: DT.amberText
        case .red: DT.redText
        case .green: DT.greenText
        case .blue: DT.blueText
        case .neutral: Color(lightWhite: 0.48, darkWhite: 0.55)
        case .provider(let kind): DT.providerText(kind)
        }
    }

    private var background: Color {
        switch tone {
        case .amber: DT.amberBg
        case .red: DT.redBg
        case .green: DT.greenBg
        case .blue: DT.blueBg
        case .neutral: DT.neutralBadgeBg
        case .provider(let kind): DT.providerBg(kind)
        }
    }

    private var stroke: Color {
        switch tone {
        case .amber: DT.amberStroke
        case .red: DT.redStroke
        case .green: DT.greenStroke
        case .blue: DT.blueBadgeStroke
        case .neutral: DT.neutralBadgeStroke
        case .provider(let kind): DT.providerStroke(kind)
        }
    }
}

extension Chip.Tone {
    static func forStatus(_ status: LaneTaskStatus) -> Chip.Tone {
        switch status {
        case .waiting: .amber
        case .running: .blue
        case .failed: .red
        case .done: .green
        case .idle: .neutral
        }
    }

    static func forOutboxKind(_ kind: OutboxKind) -> Chip.Tone {
        switch kind {
        case .approval, .nativeApproval: .amber
        case .question: .blue
        case .error: .red
        case .completion: .green
        }
    }
}

// MARK: - Countdown / share ring

/// Ring expressing "things that expire": approval reply window, undo
/// countdown, quota remaining. Equivalent of the prototype's conic pies.
struct ConicRing: View {
    /// 0…1 filled fraction.
    let fraction: Double
    let color: Color
    var size: CGFloat = 32
    var lineWidth: CGFloat = 4
    var coreBackground: Color = DT.cardStrong
    var dashed = false

    var body: some View {
        ZStack {
            Circle()
                .stroke(DT.ringTrack, style: StrokeStyle(lineWidth: lineWidth, dash: dashed ? [2.5, 2.5] : []))
            Circle()
                .trim(from: 0, to: max(0, min(1, fraction)))
                .stroke(color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .butt))
                .rotationEffect(.degrees(-90))
            Circle()
                .fill(coreBackground)
                .padding(lineWidth + 0.5)
        }
        .frame(width: size, height: size)
    }
}

// MARK: - Pill buttons

struct PillButtonStyle: ButtonStyle {
    enum Rank { case primary, secondary, tertiary, destructiveGhost }

    let rank: Rank
    var fontSize: CGFloat = 11.5
    var horizontalPadding: CGFloat = 16

    @State private var hovering = false
    @Environment(\.snapshotRendering) private var snapshotRendering

    func makeBody(configuration: Configuration) -> some View {
        let label = configuration.label
            .font(.system(size: fontSize, weight: rank == .primary ? .bold : .semibold))
            .foregroundStyle(foreground)
            .padding(.horizontal, horizontalPadding)
            .padding(.vertical, 6)

        Group {
            if snapshotRendering {
                label.background(snapshotBackground, in: Capsule())
            } else {
                label.glassEffect(glass, in: .capsule)
            }
        }
            .contentShape(Capsule())
            .onHover { hovering = $0 }
            .animation(.easeOut(duration: 0.12), value: hovering)
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
            .brightness(configuration.isPressed ? -0.05 : (hovering ? 0.04 : 0))
    }

    private var snapshotBackground: AnyShapeStyle {
        switch rank {
        case .primary:
            AnyShapeStyle(DT.primaryGradient)
        case .secondary:
            AnyShapeStyle(DT.cardStrong)
        case .tertiary:
            AnyShapeStyle(DT.cardFaint)
        case .destructiveGhost:
            AnyShapeStyle(DT.redBg)
        }
    }

    private var foreground: Color {
        switch rank {
        case .primary: .white
        case .secondary: Color(lightWhite: 0.8, darkWhite: 0.9)
        case .tertiary: Color(lightWhite: 0.55, darkWhite: 0.6)
        case .destructiveGhost: DT.redText
        }
    }

    private var glass: Glass {
        switch rank {
        case .primary: .regular.tint(DT.blue).interactive()
        case .secondary: .regular.interactive()
        case .tertiary: .clear.interactive()
        case .destructiveGhost: .regular.tint(DT.redBg).interactive()
        }
    }
}

// MARK: - Native Liquid Glass surface

struct LiquidGlassSurface: ViewModifier {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    var tint: Color?
    var radius: CGFloat
    var interactive: Bool
    var stroke: Color
    var shadow: Color
    var shadowRadius: CGFloat
    var shadowY: CGFloat

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        Group {
            if snapshotRendering {
                content.background(.ultraThinMaterial, in: shape)
            } else if interactive || !model.themeSettings.maintainsTransparencyWhenInactive {
                content.glassEffect(.regular.tint(tint).interactive(interactive), in: shape)
            } else {
                content.background {
                    ZStack {
                        AlwaysActiveVisualEffectView(
                            material: .underWindowBackground,
                            blendingMode: .withinWindow
                        )
                        if let tint {
                            tint
                        }
                    }
                    .clipShape(shape)
                }
            }
        }
            .overlay(shape.strokeBorder(stroke, lineWidth: 1))
            .shadow(color: shadow, radius: shadowRadius, y: shadowY)
    }
}

/// The three primary workspace lanes use the user-selected theme
/// transparency instead of the fixed regular-glass fill used by other cards.
struct MainLaneSurface: ViewModifier {
    @EnvironmentObject private var model: AppModel

    var radius: CGFloat
    var stroke: Color
    var shadow: Color
    var shadowRadius: CGFloat
    var shadowY: CGFloat

    func body(content: Content) -> some View {
        content.modifier(ThemedLaneSurface(
            opacity: model.themeSettings.laneOpacity,
            maintainsTransparencyWhenInactive:
                model.themeSettings.maintainsTransparencyWhenInactive,
            radius: radius,
            stroke: stroke,
            shadow: shadow,
            shadowRadius: shadowRadius,
            shadowY: shadowY
        ))
    }
}

/// Shared by the real workspace lanes and Settings preview. Liquid Glass is
/// present throughout the adjustable range, while the literal 0% endpoint
/// deliberately removes the glass layer so the surface is fully transparent.
struct ThemedLaneSurface: ViewModifier {
    @Environment(\.snapshotRendering) private var snapshotRendering

    var opacity: Double
    var maintainsTransparencyWhenInactive: Bool = true
    var radius: CGFloat
    var stroke: Color
    var shadow: Color
    var shadowRadius: CGFloat
    var shadowY: CGFloat

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: radius, style: .continuous)
        let clampedOpacity = min(1, max(0, opacity))

        Group {
            if snapshotRendering || clampedOpacity == 0 {
                content.background(shape.fill(DT.mainLaneFill(opacity: clampedOpacity)))
            } else if !maintainsTransparencyWhenInactive {
                content
                    .background(shape.fill(DT.mainLaneFill(opacity: clampedOpacity)))
                    .glassEffect(.clear, in: shape)
            } else {
                content.background {
                    ZStack {
                        AlwaysActiveVisualEffectView(
                            material: .underWindowBackground,
                            blendingMode: .withinWindow
                        )
                            .opacity(0.20)
                        shape.fill(DT.mainLaneFill(opacity: clampedOpacity))
                    }
                    .clipShape(shape)
                }
            }
        }
        .overlay(shape.strokeBorder(stroke, lineWidth: 1))
        .shadow(color: shadow.opacity(clampedOpacity), radius: shadowRadius, y: shadowY)
    }
}

// MARK: - Glass sheet card

/// Translucent sheet laid on the glassy window, following the prototype's
/// white-overlay cards (fill + hairline + inner top highlight + drop shadow).
struct SheetCard: ViewModifier {
    var fill: Color = DT.cardMedium
    var stroke: Color = DT.hairline
    var radius: CGFloat = DT.radiusCard
    var shadow: Color = .clear
    var shadowRadius: CGFloat = 0
    var shadowY: CGFloat = 0

    func body(content: Content) -> some View {
        content
            .background(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .fill(fill)
                    .overlay(
                        RoundedRectangle(cornerRadius: radius, style: .continuous)
                            .strokeBorder(DT.innerHighlight, lineWidth: 1)
                            .blendMode(.plusLighter)
                            .opacity(0.55)
                            .mask(
                                LinearGradient(
                                    colors: [.white, .clear],
                                    startPoint: .top, endPoint: .center
                                )
                            )
                    )
                    .shadow(color: shadow, radius: shadowRadius, y: shadowY)
            )
            .overlay(
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(stroke, lineWidth: 1)
            )
    }
}

extension View {
    func liquidGlassSurface(
        tint: Color? = nil,
        radius: CGFloat = DT.radiusCard,
        interactive: Bool = false,
        stroke: Color = DT.hairline,
        shadow: Color = .clear,
        shadowRadius: CGFloat = 0,
        shadowY: CGFloat = 0
    ) -> some View {
        modifier(LiquidGlassSurface(
            tint: tint,
            radius: radius,
            interactive: interactive,
            stroke: stroke,
            shadow: shadow,
            shadowRadius: shadowRadius,
            shadowY: shadowY
        ))
    }

    func sheetCard(
        fill: Color = DT.cardMedium,
        stroke: Color = DT.hairline,
        radius: CGFloat = DT.radiusCard,
        shadow: Color = .clear,
        shadowRadius: CGFloat = 0,
        shadowY: CGFloat = 0
    ) -> some View {
        modifier(SheetCard(
            fill: fill, stroke: stroke, radius: radius,
            shadow: shadow, shadowRadius: shadowRadius, shadowY: shadowY
        ))
    }

    func mainLaneSurface(
        radius: CGFloat = DT.radiusLane,
        stroke: Color = DT.hairline,
        shadow: Color = .clear,
        shadowRadius: CGFloat = 0,
        shadowY: CGFloat = 0
    ) -> some View {
        modifier(MainLaneSurface(
            radius: radius,
            stroke: stroke,
            shadow: shadow,
            shadowRadius: shadowRadius,
            shadowY: shadowY
        ))
    }

    /// Hover brightening used across cards/rows (~6% per the handoff).
    func hoverBrightens() -> some View {
        modifier(HoverBrighten())
    }
}

private struct HoverBrighten: ViewModifier {
    @State private var hovering = false

    func body(content: Content) -> some View {
        content
            .brightness(hovering ? 0.04 : 0)
            .animation(.easeOut(duration: 0.12), value: hovering)
            .onHover { hovering = $0 }
    }
}

// MARK: - Snapshot rendering flag

/// True when views render offscreen via ImageRenderer (SnapshotTool), which
/// cannot host NSViewRepresentable or scroll views.
public struct SnapshotRenderingKey: EnvironmentKey {
    public static let defaultValue = false
}

public extension EnvironmentValues {
    var snapshotRendering: Bool {
        get { self[SnapshotRenderingKey.self] }
        set { self[SnapshotRenderingKey.self] = newValue }
    }
}

// MARK: - Window background

/// AppKit's Liquid Glass view has no public way to opt out of the inactive
/// rendering it applies when a window loses focus. NSVisualEffectView does:
/// `.active` keeps the material stable without changing the real key window.
struct AlwaysActiveVisualEffectView: NSViewRepresentable {
    var material: NSVisualEffectView.Material
    var blendingMode: NSVisualEffectView.BlendingMode

    func makeNSView(context: Context) -> NSVisualEffectView {
        Self.makeView(material: material, blendingMode: blendingMode)
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {
        Self.configure(view, material: material, blendingMode: blendingMode)
    }

    @MainActor
    static func makeView(
        material: NSVisualEffectView.Material,
        blendingMode: NSVisualEffectView.BlendingMode
    ) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        configure(view, material: material, blendingMode: blendingMode)
        return view
    }

    @MainActor
    static func configure(
        _ view: NSVisualEffectView,
        material: NSVisualEffectView.Material,
        blendingMode: NSVisualEffectView.BlendingMode
    ) {
        view.material = material
        view.blendingMode = blendingMode
        view.state = .active
        view.isEmphasized = false
    }
}

/// Native Liquid Glass behavior used when the user allows materials to follow
/// the active state of the containing window.
struct WindowGlass: NSViewRepresentable {
    func makeNSView(context: Context) -> NSGlassEffectView {
        let view = NSGlassEffectView()
        view.style = .regular
        view.cornerRadius = 0
        view.tintColor = NSColor.windowBackgroundColor.withAlphaComponent(0.08)
        return view
    }

    func updateNSView(_ view: NSGlassEffectView, context: Context) {
        view.style = .regular
        view.cornerRadius = 0
    }
}

/// Loads a persisted custom background once per selected URL instead of
/// decoding it again on every one-second AppModel update.
struct AppThemeImage: View {
    let url: URL
    @State private var image: NSImage?

    var body: some View {
        GeometryReader { proxy in
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .scaledToFill()
                    .frame(width: proxy.size.width, height: proxy.size.height)
                    .clipped()
            }
        }
        .task(id: url) {
            image = NSImage(contentsOf: url)
        }
    }
}

struct WindowGlassBackground: ViewModifier {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering

    func body(content: Content) -> some View {
        content
            .background(
                ZStack {
                    if let url = model.themeBackgroundURL {
                        AppThemeBackdrop(
                            url: url,
                            kind: model.themeSettings.backgroundKind,
                            maintainsTransparencyWhenInactive:
                                model.themeSettings.maintainsTransparencyWhenInactive
                        )
                    } else if !snapshotRendering {
                        if model.themeSettings.maintainsTransparencyWhenInactive {
                            AlwaysActiveVisualEffectView(
                                material: .underWindowBackground,
                                blendingMode: .behindWindow
                            )
                        } else {
                            WindowGlass()
                        }
                        AppThemeTint()
                    } else {
                        Rectangle().fill(.ultraThinMaterial)
                        AppThemeTint()
                    }
                }
                .ignoresSafeArea()
            )
    }
}

/// The exact custom-background stack shared by the main window and the theme
/// preview, so crop, material, and tint cannot drift between the two.
struct AppThemeBackdrop: View {
    let url: URL
    let kind: ThemeBackgroundKind
    var maintainsTransparencyWhenInactive: Bool = true
    @Environment(\.snapshotRendering) private var snapshotRendering

    var body: some View {
        ZStack {
            AppThemeMediaView(url: url, kind: kind)
            if snapshotRendering {
                Rectangle()
                    .fill(.ultraThinMaterial)
                    .opacity(0.58)
            } else if maintainsTransparencyWhenInactive {
                AlwaysActiveVisualEffectView(
                    material: .underWindowBackground,
                    blendingMode: .withinWindow
                )
                    .opacity(0.58)
            } else {
                Rectangle()
                    .fill(.ultraThinMaterial)
                    .opacity(0.58)
            }
            AppThemeTint()
        }
    }
}

private struct AppThemeTint: View {
    var body: some View {
        LinearGradient(
            colors: [
                DT.logoTint.opacity(0.055),
                Color.clear,
                DT.greenDot.opacity(0.035),
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }
}

// MARK: - Status dot

struct StatusDot: View {
    let color: Color
    var size: CGFloat = 7
    var glow = false

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: size, height: size)
            .shadow(color: glow ? color.opacity(0.8) : .clear, radius: glow ? 4 : 0)
    }
}
