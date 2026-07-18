import ActRealmKit
import SwiftUI

// MARK: - Logo

/// Three rounded vertical bars — the flow-agent mark.
struct LogoMark: View {
    var barWidth: CGFloat = 3
    var heights: [CGFloat] = [6, 11, 8]
    var color: Color = DT.logoTint

    var body: some View {
        HStack(alignment: .bottom, spacing: 2) {
            ForEach(heights.indices, id: \.self) { index in
                RoundedRectangle(cornerRadius: 2)
                    .fill(color)
                    .frame(width: barWidth, height: heights[index])
            }
        }
    }
}

/// Compact toolbar app icon. The filled tile gives the mark a clear icon
/// silhouette while the unified title bar supplies the native glass layer.
struct AppMark: View {
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(DT.logoTint.gradient)
            LogoMark(barWidth: 2.7, heights: [7, 14, 10], color: .white)
        }
        .frame(width: 27, height: 27)
        .shadow(color: DT.logoTint.opacity(0.2), radius: 4, y: 2)
        .accessibilityHidden(true)
    }
}

// MARK: - Provider avatar

struct ProviderAvatar: View {
    let kind: ProviderKind
    var size: CGFloat = 24

    var body: some View {
        Circle()
            .fill(DT.providerBg(kind))
            .overlay(Circle().strokeBorder(DT.providerStroke(kind), lineWidth: 1))
            .overlay(
                Text(kind.avatarLetter)
                    .font(.system(size: size * 0.42, weight: .heavy))
                    .foregroundStyle(DT.providerText(kind))
            )
            .frame(width: size, height: size)
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
        case .approval: .amber
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
            } else {
                content.glassEffect(.regular.tint(tint).interactive(interactive), in: shape)
            }
        }
            .overlay(shape.strokeBorder(stroke, lineWidth: 1))
            .shadow(color: shadow, radius: shadowRadius, y: shadowY)
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

/// Behind-window blur so the whole window reads as one glass sheet.
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

struct WindowGlassBackground: ViewModifier {
    @Environment(\.snapshotRendering) private var snapshotRendering

    func body(content: Content) -> some View {
        content
            .background(
                ZStack {
                    if !snapshotRendering {
                        WindowGlass()
                    } else {
                        Rectangle().fill(.ultraThinMaterial)
                    }
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
                .ignoresSafeArea()
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
