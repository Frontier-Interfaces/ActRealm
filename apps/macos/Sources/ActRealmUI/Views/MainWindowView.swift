import ActRealmKit
import SwiftUI

public enum MainWindowPage: Sendable, Hashable {
    case actRoom
    case foregroundScheduling
}

/// Native SwiftUI reproduction of `Flow Agent Interactive Demo.dc.html`.
/// The hidden native title bar keeps real macOS traffic-light controls while
/// this view supplies the interaction model's floating glass header.
public struct MainWindowView: View {
    public init(initialPage: MainWindowPage = .actRoom) {
        _page = State(initialValue: initialPage)
    }

    @EnvironmentObject private var model: AppModel
    @State private var showingRuntimeMonitor = false
    @State private var page: MainWindowPage

    public var body: some View {
        VStack(spacing: 0) {
            WindowHeader(
                page: page,
                openScheduling: { switchPage(to: .foregroundScheduling) },
                openRuntime: { showingRuntimeMonitor = true }
            )
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16)
                .padding(.top, 16)

            Group {
                if page == .actRoom {
                    GeometryReader { proxy in
                        let gap: CGFloat = 16
                        let available = max(0, proxy.size.width - gap * 2)

                        HStack(spacing: gap) {
                            OutboxSection()
                                .frame(width: available * 0.30)
                            AgentTasksSection()
                                .frame(width: available * 0.48)
                            QuotaSection { showingRuntimeMonitor = true }
                                .frame(width: available * 0.22)
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 14)
                    .padding(.bottom, 14)
                    .transition(.opacity.combined(with: .scale(scale: 0.99)))
                } else {
                    ForegroundSchedulingView {
                        switchPage(to: .actRoom)
                    }
                    .transition(.opacity.combined(with: .move(edge: .trailing)))
                }
            }
        }
        .frame(
            minWidth: 1120,
            idealWidth: 1440,
            maxWidth: .infinity,
            minHeight: 650,
            idealHeight: 820,
            maxHeight: .infinity
        )
        .modifier(WindowGlassBackground())
        .sheet(isPresented: $showingRuntimeMonitor) {
            RuntimeMonitorView().environmentObject(model)
        }
        .overlay(alignment: .bottom) {
            if let toast = model.toastMessage {
                StatusToast(text: toast)
                    .padding(.bottom, 20)
                    .transition(.move(edge: .bottom).combined(with: .opacity))
                    .allowsHitTesting(false)
            }
        }
        .animation(.easeOut(duration: 0.22), value: model.toastMessage)
        .animation(.easeOut(duration: 0.25), value: page)
        .onChange(of: model.foregroundDispatch?.phase) { _, phase in
            if phase == .returnedToActRoom {
                switchPage(to: .actRoom)
            }
        }
    }

    private func switchPage(to destination: MainWindowPage) {
        guard page != destination else { return }
        page = destination
    }
}

private struct WindowHeader: View {
    @EnvironmentObject private var model: AppModel
    let page: MainWindowPage
    let openScheduling: () -> Void
    let openRuntime: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            LogoMark(barWidth: 3, heights: [6, 11, 8])
                .frame(width: 14, height: 12, alignment: .bottom)
            Text("ActRealm")
                .font(.system(size: 14, weight: .bold))
                .foregroundStyle(DT.textStrong)
            Spacer(minLength: 20)
            SchedulingNavigationButton(
                selected: page == .foregroundScheduling,
                action: openScheduling
            )
            BridgeChip(action: openRuntime)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(height: 42)
        .liquidGlassSurface(
            tint: DT.cardSoft.opacity(0.2),
            radius: 16,
            interactive: false,
            stroke: DT.hairline,
            shadow: DT.cardShadow.opacity(0.65),
            shadowRadius: 15,
            shadowY: 6
        )
    }
}

private struct SchedulingNavigationButton: View {
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Label("台前调度", systemImage: "rectangle.3.group")
                .font(.system(size: 10.5, weight: .bold))
                .foregroundStyle(selected ? Color.white : DT.textSecondary)
                .padding(.horizontal, 11)
                .padding(.vertical, 4)
                .background(
                    selected ? AnyShapeStyle(DT.primaryGradient) : AnyShapeStyle(DT.cardMedium),
                    in: Capsule()
                )
                .overlay(
                    Capsule().strokeBorder(
                        selected ? DT.blueBadgeStroke : DT.neutralBadgeStroke,
                        lineWidth: 1
                    )
                )
                .shadow(color: selected ? DT.blue.opacity(0.22) : .clear, radius: 5, y: 2)
        }
        .buttonStyle(.plain)
        .focusable(false)
        .help(selected ? "当前正在查看台前调度" : "打开台前调度")
    }
}

private struct BridgeChip: View {
    @EnvironmentObject private var model: AppModel
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                StatusDot(color: dotColor, size: 6)
                Text(label)
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(textColor)
            }
            .padding(.horizontal, 11)
            .padding(.vertical, 4)
            .background(tint, in: Capsule())
            .overlay(Capsule().strokeBorder(stroke, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .focusable(false)
        .help("打开 Runtime 监控、诊断与重启")
    }

    private var label: String {
        switch model.bridgeStatus {
        case .listening: "bridge.sock"
        case .starting: "启动中…"
        case .absent: "Runtime 未连接"
        }
    }

    private var dotColor: Color {
        switch model.bridgeStatus {
        case .listening: DT.greenDot
        case .starting: DT.amberDot
        case .absent: DT.redText
        }
    }

    private var textColor: Color {
        switch model.bridgeStatus {
        case .listening: DT.greenText
        case .starting: DT.amberText
        case .absent: DT.redText
        }
    }

    private var tint: Color {
        switch model.bridgeStatus {
        case .listening: DT.greenBg
        case .starting: DT.amberBg
        case .absent: DT.redBg
        }
    }

    private var stroke: Color {
        switch model.bridgeStatus {
        case .listening: DT.greenStroke
        case .starting: DT.amberStroke
        case .absent: DT.redStroke
        }
    }
}

private struct StatusToast: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 11.5, weight: .semibold))
            .foregroundStyle(Color.white.opacity(0.92))
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(Color(red: 20 / 255, green: 22 / 255, blue: 30 / 255).opacity(0.85), in: Capsule())
            .overlay(Capsule().strokeBorder(Color.white.opacity(0.14), lineWidth: 1))
            .shadow(color: .black.opacity(0.3), radius: 15, y: 8)
    }
}
