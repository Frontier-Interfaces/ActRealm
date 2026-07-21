import AppKit
import ActRealmKit
import SwiftUI

public enum MainWindowPage: Sendable, Hashable {
    case actRealmWorkspace
    case agentSetup
    case foregroundScheduling
}

/// Native SwiftUI reproduction of `ActRealm Interactive Demo.dc.html`.
/// The native title bar is transparent: macOS keeps ownership of the window
/// controls while the app header shares the same uninterrupted background.
public struct MainWindowView: View {
    public init(initialPage: MainWindowPage = .actRealmWorkspace) {
        _page = State(initialValue: initialPage)
    }

    @EnvironmentObject private var model: AppModel
    @Environment(\.openWindow) private var openWindow
    @State private var page: MainWindowPage

    public var body: some View {
        VStack(spacing: 0) {
            IntegratedWindowHeader(
                page: page,
                openWorkspace: { switchPage(to: .actRealmWorkspace) },
                openSetup: { switchPage(to: .agentSetup) },
                openScheduling: { switchPage(to: .foregroundScheduling) },
                openSettings: { openWindow(id: "settings") }
            )

            Group {
                if page == .actRealmWorkspace {
                    GeometryReader { proxy in
                        let gap: CGFloat = 16
                        let available = max(0, proxy.size.width - gap * 2)

                        HStack(spacing: gap) {
                            OutboxSection()
                                .frame(width: available * 0.30)
                            AgentTasksSection(
                                onOpenSetup: { switchPage(to: .agentSetup) }
                            )
                                .frame(width: available * 0.48)
                            QuotaSection()
                                .frame(width: available * 0.22)
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 14)
                    .padding(.bottom, 14)
                    .transition(.opacity.combined(with: .scale(scale: 0.99)))
                } else if page == .agentSetup {
                    AgentSetupView {
                        switchPage(to: .actRealmWorkspace)
                    }
                    .transition(.opacity.combined(with: .move(edge: .trailing)))
                } else {
                    ForegroundSchedulingView {
                        switchPage(to: .actRealmWorkspace)
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
        .background {
            GeometryReader { proxy in
                Color.clear
                    .onAppear {
                        model.updateMainWindowSize(proxy.size)
                    }
                    .onChange(of: proxy.size) { _, size in
                        model.updateMainWindowSize(size)
                    }
            }
        }
        .modifier(WindowGlassBackground())
        .ignoresSafeArea(.container, edges: .top)
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
            if phase == .returnedToActRealmWorkspace {
                switchPage(to: .actRealmWorkspace)
            }
        }
        .onChange(of: model.notificationPulse) { _, _ in
            guard model.uiSettings.soundEnabled else { return }
            NSSound.beep()
        }
    }

    private func switchPage(to destination: MainWindowPage) {
        guard page != destination else { return }
        page = destination
    }
}

private struct IntegratedWindowHeader: View {
    @EnvironmentObject private var model: AppModel
    let page: MainWindowPage
    let openWorkspace: () -> Void
    let openSetup: () -> Void
    let openScheduling: () -> Void
    let openSettings: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            WindowBrand()
            if shouldShowAgentSetupNotice {
                AgentConnectionButton(action: openSetup)
            }
            Spacer(minLength: 20)

            if page != .actRealmWorkspace {
                Button(action: openWorkspace) {
                    Label("工作区", systemImage: "square.grid.3x3")
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(DT.textSecondary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 4)
                        .background(DT.cardMedium, in: Capsule())
                        .overlay(Capsule().strokeBorder(DT.neutralBadgeStroke, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .help("返回 ActRealm 工作区")
            }

            SchedulingNavigationButton(
                selected: page == .foregroundScheduling,
                action: openScheduling
            )

            Button(action: openSettings) {
                Label("设置", systemImage: "gearshape")
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textSecondary)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(DT.cardMedium, in: Capsule())
                    .overlay(Capsule().strokeBorder(DT.neutralBadgeStroke, lineWidth: 1))
            }
            .buttonStyle(.plain)
            .focusable(false)
            .help("打开 ActRealm 设置")

            if !model.bridgeStatus.isListening {
                Button(action: openSettings) {
                    Label(runtimeIssueLabel, systemImage: "exclamationmark.triangle.fill")
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(DT.redText)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 4)
                        .background(DT.redBg, in: Capsule())
                        .overlay(Capsule().strokeBorder(DT.redStroke, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .help("打开设置检查本机服务")
            }
        }
        .padding(.leading, 82)
        .padding(.trailing, 16)
        .frame(height: 34)
        .contentShape(Rectangle())
    }

    private var shouldShowAgentSetupNotice: Bool {
        model.setupInfo == nil || model.isFirstRun || model.pendingAgentSetupCount > 0
    }

    private var runtimeIssueLabel: String {
        switch model.bridgeStatus {
        case .starting: "服务启动中"
        case .absent: "服务未连接"
        case .listening: ""
        }
    }
}

private struct WindowBrand: View {
    var body: some View {
        HStack(spacing: 7) {
            LogoMark(barWidth: 3, heights: [6, 11, 8])
                .frame(width: 14, height: 12, alignment: .bottom)
            Text("ActRealm")
                .font(.system(size: 14, weight: .bold))
                .foregroundStyle(DT.textStrong)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("ActRealm")
    }
}

private struct AgentConnectionButton: View {
    @EnvironmentObject private var model: AppModel
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                StatusDot(color: color, size: 6, glow: model.connectedAgentCount > 0)
                Text(label)
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(textColor)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(background, in: Capsule())
            .overlay(Capsule().strokeBorder(stroke, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .help("打开 Agent 接入中心")
    }

    private var label: String {
        guard model.setupInfo != nil else { return "正在检测 Agent" }
        if model.isFirstRun { return "未连接 Agent" }
        if model.pendingAgentSetupCount > 0 {
            return model.connectedAgentCount > 0
                ? "\(model.connectedAgentCount) 个已接入 · \(model.pendingAgentSetupCount) 待处理"
                : "\(model.pendingAgentSetupCount) 项接入待处理"
        }
        return "管理 Agent"
    }

    private var color: Color {
        model.pendingAgentSetupCount > 0 || model.isFirstRun ? DT.amberDot
            : model.connectedAgentCount > 0 ? DT.greenDot : DT.textFaint
    }
    private var textColor: Color {
        model.pendingAgentSetupCount > 0 || model.isFirstRun ? DT.amberText
            : model.connectedAgentCount > 0 ? DT.greenText : DT.textWeak
    }
    private var background: Color {
        model.pendingAgentSetupCount > 0 || model.isFirstRun ? DT.amberBg
            : model.connectedAgentCount > 0 ? DT.greenBg : DT.cardFaint
    }
    private var stroke: Color {
        model.pendingAgentSetupCount > 0 || model.isFirstRun ? DT.amberStroke
            : model.connectedAgentCount > 0 ? DT.greenStroke : DT.hairline
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
