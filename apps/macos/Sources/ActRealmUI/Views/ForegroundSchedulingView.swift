import ActRealmKit
import SwiftUI

/// Native SwiftUI reproduction of the reference HTML's 台前调度 management
/// screen. Every edit is written through AppModel so the selected policy is
/// durable and immediately reflected by the rule preview.
public struct ForegroundSchedulingView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    private let onBack: () -> Void

    public init(onBack: @escaping () -> Void) {
        self.onBack = onBack
    }

    public var body: some View {
        Group {
            if snapshotRendering {
                pageContent
            } else {
                ScrollView {
                    pageContent
                }
                .scrollIndicators(.visible)
            }
        }
    }

    private var pageContent: some View {
        VStack(spacing: 14) {
            pageHeading
            masterSwitches

            Group {
                workspaceStatus
                arrivalStrategies
                afterOpening
            }
            .opacity(settings.isEnabled ? 1 : 0.45)
            .allowsHitTesting(settings.isEnabled)

            rulePreview

            providerRules
                .opacity(settings.isEnabled ? 1 : 0.45)
                .allowsHitTesting(settings.isEnabled)
        }
        .frame(maxWidth: 1060)
        .padding(.horizontal, 20)
        .padding(.top, 16)
        .padding(.bottom, 60)
        .frame(maxWidth: .infinity)
    }

    private var settings: ForegroundSchedulingSettings {
        model.foregroundScheduling
    }

    // MARK: - Heading and master switches

    private var pageHeading: some View {
        HStack(alignment: .top, spacing: 13) {
            Button(action: onBack) {
                Image(systemName: "arrow.left")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                    .frame(width: 36, height: 36)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .liquidGlassSurface(
                tint: DT.cardSoft.opacity(0.25),
                radius: 12,
                interactive: true,
                stroke: DT.hairline,
                shadow: DT.softShadow,
                shadowRadius: 8,
                shadowY: 2
            )
            .help("返回 ActRealm 工作区")

            VStack(alignment: .leading, spacing: 4) {
                Text("台前调度")
                    .font(.system(size: 23, weight: .heavy))
                    .foregroundStyle(DT.textStrong)
                Text("决定 Agent 什么时候进入前台，以及何时把控制权还给 ActRealm 工作区")
                    .font(DT.body(12.5))
                    .foregroundStyle(DT.textSecondary)
            }

            Spacer(minLength: 16)

            HStack(spacing: 7) {
                StatusDot(color: pageStatusColor, size: 7, glow: pageStatusGlow)
                Text(pageStatusText)
                    .font(.system(size: 11, weight: .bold))
            }
            .foregroundStyle(pageStatusForeground)
            .padding(.horizontal, 13)
            .padding(.vertical, 6)
            .background(pageStatusBackground, in: Capsule())
            .overlay(Capsule().strokeBorder(pageStatusStroke, lineWidth: 1))
            .padding(.top, 5)
        }
        .padding(2)
    }

    private var pageStatusText: String {
        if !settings.isEnabled { return "当前：调度已关闭" }
        if settings.workspaceApps.isEmpty { return "当前：待绑定桌面" }
        return "当前：运行正常"
    }

    private var pageStatusColor: Color {
        settings.isEnabled && !settings.workspaceApps.isEmpty ? DT.greenDot : DT.amberDot
    }

    private var pageStatusGlow: Bool {
        settings.isEnabled && !settings.workspaceApps.isEmpty
    }

    private var pageStatusForeground: Color {
        settings.isEnabled && !settings.workspaceApps.isEmpty ? DT.greenText : DT.amberText
    }

    private var pageStatusBackground: Color {
        settings.isEnabled && !settings.workspaceApps.isEmpty ? DT.greenBg : DT.amberBg
    }

    private var pageStatusStroke: Color {
        settings.isEnabled && !settings.workspaceApps.isEmpty ? DT.greenStroke : DT.amberStroke
    }

    private var masterSwitches: some View {
        SchedulingPageCard(padding: 0) {
            VStack(spacing: 0) {
                SchedulingToggleRow(
                    title: "启用台前调度",
                    detail: "关闭后不自动切换窗口，新任务只进入待处理列表。",
                    isOn: binding(\.isEnabled)
                )

                Divider()
                    .overlay(DT.separator)
                    .padding(.horizontal, 18)

                SchedulingToggleRow(
                    title: "切换完成后自动关闭调度",
                    detail: "页面成功切换（已接收）后自动关闭台前调度，避免反复切换窗口",
                    isOn: binding(\.closesAfterAcceptance)
                )
                .opacity(settings.isEnabled ? 1 : 0.45)
                .allowsHitTesting(settings.isEnabled)
            }
        }
    }

    // MARK: - Workspace status

    private var workspaceStatus: some View {
        let workspace = model.foregroundWorkspaceStatus
        return SchedulingPageCard {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 10) {
                    Text("调度工作桌面")
                        .font(.system(size: 13, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("绑定放置协作应用的虚拟桌面")
                        .font(DT.body(11))
                        .foregroundStyle(DT.textWeak)
                    Spacer()
                    if settings.workspaceApps.isEmpty {
                        Text("尚未绑定")
                            .font(.system(size: 10.5, weight: .bold))
                            .foregroundStyle(DT.amberText)
                    } else {
                        WorkspaceReadinessPill(status: workspace)
                    }
                }

                if settings.workspaceApps.isEmpty {
                    HStack(alignment: .center, spacing: 16) {
                        VStack(alignment: .leading, spacing: 5) {
                            Text("先选择协作桌面")
                                .font(.system(size: 13, weight: .bold))
                                .foregroundStyle(DT.textPrimary)
                            Text("选择窗口会跟随虚拟桌面显示。切换到放置 Claude、Codex、Chrome 等应用的桌面后，绑定当前桌面即可。")
                                .font(DT.body(11))
                                .foregroundStyle(DT.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer()
                        Button("选择协作桌面…") {
                            model.beginForegroundWorkspaceSelection()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                } else {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack(spacing: 8) {
                            ForEach(settings.workspaceApps) { application in
                                Text(application.name)
                                    .font(.system(size: 10.5, weight: .semibold))
                                    .foregroundStyle(DT.textSecondary)
                                    .padding(.horizontal, 9)
                                    .padding(.vertical, 4)
                                    .background(DT.neutralBadgeBg, in: Capsule())
                            }
                            Spacer()
                        }

                        HStack {
                            Label(
                                workspace.isSchedulingWorkspaceActive ? "当前位于协作桌面" : "当前位于其他桌面",
                                systemImage: workspace.isSchedulingWorkspaceActive ? "checkmark.circle.fill" : "circle.dashed"
                            )
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(workspace.isSchedulingWorkspaceActive ? DT.greenText : DT.textWeak)
                            Spacer()
                            Button("测试调度") { model.testForegroundScheduling() }
                            Button("重新选择…") { model.beginForegroundWorkspaceSelection() }
                            Button("清除绑定", role: .destructive) { model.clearForegroundWorkspaceBinding() }
                        }
                    }
                }
            }
        }
    }

    // MARK: - Arrival strategies

    private var arrivalStrategies: some View {
        SchedulingPageCard {
            VStack(alignment: .leading, spacing: 15) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("当任务需要处理时")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("三种到达策略互斥，选一个")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                }

                HStack(alignment: .top, spacing: 12) {
                    StrategyCard(
                        strategy: .immediate,
                        selected: settings.strategy == .immediate,
                        title: "立即打开 Agent",
                        detail: "任务需要处理时，直接在协作桌面打开对应 Agent。",
                        steps: [
                            FlowToken("任务到达", tone: .neutral),
                            FlowToken("Agent 前台", tone: .blue),
                            FlowToken("等待接收", tone: .green),
                        ],
                        recommended: false,
                        reminderSeconds: nil,
                        onSelect: { chooseStrategy(.immediate) },
                        onSeconds: nil
                    )

                    StrategyCard(
                        strategy: .remind,
                        selected: settings.strategy == .remind,
                        title: "先提醒，再打开",
                        detail: "ActRealm 工作区先提醒；没有处理时，再自动打开 Agent。",
                        steps: [
                            FlowToken("任务到达", tone: .neutral),
                            FlowToken("提醒倒计时", tone: .amber),
                            FlowToken("Agent 前台", tone: .blue),
                            FlowToken("等待接收", tone: .green),
                        ],
                        recommended: true,
                        reminderSeconds: settings.strategy == .remind ? settings.reminderSeconds : nil,
                        onSelect: { chooseStrategy(.remind) },
                        onSeconds: { seconds in
                            model.updateForegroundScheduling { $0.reminderSeconds = seconds }
                        }
                    )

                    StrategyCard(
                        strategy: .actRealmWorkspace,
                        selected: settings.strategy == .actRealmWorkspace,
                        title: "只留在 ActRealm 工作区",
                        detail: "任务只进入待处理列表，不自动切换窗口。",
                        steps: [
                            FlowToken("任务到达", tone: .neutral),
                            FlowToken("待处理列表", tone: .blue),
                            FlowToken("手动打开", tone: .green),
                        ],
                        recommended: false,
                        reminderSeconds: nil,
                        onSelect: { chooseStrategy(.actRealmWorkspace) },
                        onSeconds: nil
                    )
                }
            }
        }
    }

    // MARK: - After opening

    private var afterOpening: some View {
        let available = settings.strategy != .actRealmWorkspace
        return SchedulingPageCard {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("Agent 打开之后")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("「立即打开」「先提醒」两种模式共用的打开后行为")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                    Spacer()
                    if !available {
                        Text("「只留在 ActRealm 工作区」无需此行为")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(DT.textWeak)
                            .padding(.horizontal, 11)
                            .padding(.vertical, 4)
                            .background(DT.neutralBadgeBg, in: Capsule())
                            .overlay(Capsule().strokeBorder(DT.neutralBadgeStroke, lineWidth: 1))
                    }
                }

                VStack(alignment: .leading, spacing: 10) {
                    SettingsInsetRow {
                        HStack(spacing: 14) {
                            VStack(alignment: .leading, spacing: 3) {
                                Text("自动返回 ActRealm 工作区")
                                    .font(.system(size: 12.5, weight: .bold))
                                    .foregroundStyle(DT.textPrimary)
                                Text("Agent 打开后若未被接收，自动把控制权交还 ActRealm 工作区")
                                    .font(DT.body(10.5))
                                    .foregroundStyle(DT.textWeak)
                            }
                            Spacer()
                            SchedulingSwitch(isOn: binding(\.returnsToActRealmWorkspace))
                        }
                    }

                    SettingsInsetRow {
                        HStack(spacing: 14) {
                            VStack(alignment: .leading, spacing: 3) {
                                Text("等待协作桌面出现")
                                    .font(.system(size: 12.5, weight: .bold))
                                    .foregroundStyle(DT.textPrimary)
                                Text("超时仍未检测到已绑定应用，则恢复 ActRealm 工作区")
                                    .font(DT.body(10.5))
                                    .foregroundStyle(DT.textWeak)
                            }
                            Spacer()
                            SecondsPicker(
                                selected: settings.acceptanceSeconds,
                                onSelect: { seconds in
                                    model.updateForegroundScheduling { $0.acceptanceSeconds = seconds }
                                }
                            )
                        }
                    }
                    .opacity(settings.returnsToActRealmWorkspace ? 1 : 0.4)
                    .allowsHitTesting(settings.returnsToActRealmWorkspace)

                }
                .opacity(available ? 1 : 0.5)
                .allowsHitTesting(available)
            }
        }
    }

    // MARK: - Rule preview

    private var rulePreview: some View {
        SchedulingPageCard {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("当前规则预览")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("随上面的选择实时更新")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                }

                RulePreviewContent(settings: settings)
                    .animation(.easeOut(duration: 0.22), value: settings)
            }
        }
    }

    // MARK: - Provider overrides

    private var providerRules: some View {
        SchedulingPageCard {
            VStack(alignment: .leading, spacing: 13) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("Agent 单独规则")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("覆盖全局策略，仅对该 Agent 生效")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                }

                VStack(spacing: 9) {
                    ProviderRuleRow(
                        provider: .codex,
                        name: "Codex CLI",
                        globalStrategy: settings.strategy,
                        selected: settings.codexRule,
                        onSelect: { rule in
                            model.updateForegroundScheduling { $0.codexRule = rule }
                        }
                    )
                    ProviderRuleRow(
                        provider: .claude,
                        name: "Claude Code",
                        globalStrategy: settings.strategy,
                        selected: settings.claudeRule,
                        onSelect: { rule in
                            model.updateForegroundScheduling { $0.claudeRule = rule }
                        }
                    )
                }
            }
        }
    }

    private func chooseStrategy(_ strategy: ForegroundArrivalStrategy) {
        model.updateForegroundScheduling { $0.strategy = strategy }
        let label: String
        switch strategy {
        case .immediate: label = "立即打开 Agent"
        case .remind: label = "先提醒，再打开"
        case .actRealmWorkspace: label = "只留在 ActRealm 工作区"
        }
        model.showToast("到达策略：\(label)")
    }

    private func binding<Value>(
        _ keyPath: WritableKeyPath<ForegroundSchedulingSettings, Value>
    ) -> Binding<Value> {
        Binding(
            get: { model.foregroundScheduling[keyPath: keyPath] },
            set: { value in
                model.updateForegroundScheduling { $0[keyPath: keyPath] = value }
            }
        )
    }
}

// MARK: - Shared page chrome

private struct SchedulingPageCard<Content: View>: View {
    var padding: CGFloat = 18
    @ViewBuilder let content: Content

    var body: some View {
        content
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .liquidGlassSurface(
                tint: DT.cardSoft.opacity(0.18),
                radius: 22,
                interactive: false,
                stroke: DT.hairline,
                shadow: DT.cardShadow.opacity(0.58),
                shadowRadius: 15,
                shadowY: 6
            )
    }
}

private struct SchedulingToggleRow: View {
    let title: String
    let detail: String
    @Binding var isOn: Bool

    var body: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.system(size: 13.5, weight: .heavy))
                    .foregroundStyle(DT.textPrimary)
                Text(detail)
                    .font(DT.body(11))
                    .foregroundStyle(DT.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            SchedulingSwitch(isOn: $isOn)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 13)
    }
}

private struct SchedulingSwitch: View {
    @Binding var isOn: Bool

    var body: some View {
        Button {
            isOn.toggle()
        } label: {
            ZStack {
                Capsule()
                    .fill(isOn ? DT.greenDot : DT.neutralBadgeStroke)
                    .frame(width: 46, height: 27)
                Circle()
                    .fill(Color.white)
                    .frame(width: 21, height: 21)
                    .shadow(color: Color.black.opacity(0.25), radius: 3, y: 1)
                    .offset(x: isOn ? 9.5 : -9.5)
            }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .animation(.spring(response: 0.28, dampingFraction: 0.72), value: isOn)
        .accessibilityValue(isOn ? "开启" : "关闭")
    }
}

private struct WorkspaceMetric<Content: View>: View {
    let title: String
    @ViewBuilder let value: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased())
                .font(.system(size: 9.5, weight: .bold))
                .kerning(0.5)
                .foregroundStyle(DT.textWeak)
            value
                .font(.system(size: 12.5, weight: .bold))
                .foregroundStyle(DT.textPrimary)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 13)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .sheetCard(
            fill: DT.cardMedium,
            stroke: DT.hairline,
            radius: 14,
            shadow: DT.softShadow,
            shadowRadius: 3,
            shadowY: 1
        )
    }
}

private struct WorkspaceAvailabilityValue: View {
    let available: Bool
    let readyText: String
    let unavailableText: String

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: available ? "checkmark" : "exclamationmark")
                .font(.system(size: 8, weight: .heavy))
                .foregroundStyle(available ? DT.greenText : DT.amberText)
                .frame(width: 17, height: 17)
                .background(available ? DT.greenBg : DT.amberBg, in: Circle())
                .overlay(Circle().strokeBorder(available ? DT.greenStroke : DT.amberStroke, lineWidth: 1))
            Text(available ? readyText : unavailableText)
                .foregroundStyle(available ? DT.textPrimary : DT.amberText)
        }
    }
}

private struct WorkspaceReadinessPill: View {
    let status: ForegroundWorkspaceStatus

    var body: some View {
        HStack(spacing: 8) {
            StatusDot(color: dotColor, size: 8, glow: isReady)
            Text(label)
                .font(.system(size: 11.5, weight: .bold))
        }
        .foregroundStyle(textColor)
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
        .background(background, in: Capsule())
        .overlay(Capsule().strokeBorder(stroke, lineWidth: 1))
    }

    private var isReady: Bool { status.isActRealmWorkspaceReady && status.isAgentAvailable }

    private var label: String {
        if !status.isActRealmWorkspaceReady { return "ActRealm 工作区尚未就位" }
        if !status.isAgentAvailable { return "目标 Agent 当前未运行" }
        return "台前调度已就绪"
    }

    private var dotColor: Color {
        if !status.isActRealmWorkspaceReady { return DT.redText }
        return isReady ? DT.greenDot : DT.amberDot
    }

    private var textColor: Color {
        if !status.isActRealmWorkspaceReady { return DT.redText }
        return isReady ? DT.greenText : DT.amberText
    }

    private var background: Color {
        if !status.isActRealmWorkspaceReady { return DT.redBg }
        return isReady ? DT.greenBg : DT.amberBg
    }

    private var stroke: Color {
        if !status.isActRealmWorkspaceReady { return DT.redStroke }
        return isReady ? DT.greenStroke : DT.amberStroke
    }
}

// MARK: - Strategy cards

private struct StrategyCard: View {
    let strategy: ForegroundArrivalStrategy
    let selected: Bool
    let title: String
    let detail: String
    let steps: [FlowToken]
    let recommended: Bool
    let reminderSeconds: Int?
    let onSelect: () -> Void
    let onSeconds: ((Int) -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 9) {
                ZStack {
                    Circle()
                        .strokeBorder(selected ? DT.blue : DT.textSecondary, lineWidth: 2)
                    Circle()
                        .fill(DT.blue)
                        .padding(5)
                        .scaleEffect(selected ? 1 : 0)
                }
                .frame(width: 19, height: 19)

                Text(title)
                    .font(.system(size: 13.5, weight: .heavy))
                    .foregroundStyle(DT.textPrimary)
            }

            Text(detail)
                .font(DT.body(11.5))
                .foregroundStyle(DT.textSecondary)
                .lineSpacing(2)
                .fixedSize(horizontal: false, vertical: true)
                .frame(minHeight: 38, alignment: .topLeading)
                .padding(.top, 9)

            TokenFlow(tokens: steps, compact: true)
                .padding(.top, 10)

            FlowTrack()
                .padding(.top, 11)

            if let reminderSeconds, let onSeconds {
                HStack(spacing: 8) {
                    Text("提醒时长")
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(DT.textSecondary)
                    Spacer()
                    SecondsPicker(selected: reminderSeconds, onSelect: onSeconds)
                }
                .padding(.top, 11)
                .overlay(alignment: .top) {
                    Rectangle()
                        .fill(DT.separator)
                        .frame(height: 1)
                }
                .padding(.top, 11)
            }
        }
        .padding(15)
        .frame(maxWidth: .infinity, minHeight: selected && strategy == .remind ? 211 : 172, alignment: .topLeading)
        .background(cardBackground, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .strokeBorder(selected ? DT.logoTint.opacity(0.75) : DT.hairline, lineWidth: selected ? 1.5 : 1)
        )
        .shadow(color: selected ? DT.logoTint.opacity(0.2) : DT.softShadow, radius: selected ? 16 : 3, y: selected ? 8 : 1)
        .offset(y: selected ? -2 : 0)
        .overlay(alignment: .topTrailing) {
            if recommended {
                Text("推荐")
                    .font(.system(size: 9.5, weight: .heavy))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 2)
                    .background(DT.primaryGradient, in: Capsule())
                    .shadow(color: DT.blue.opacity(0.3), radius: 5, y: 2)
                    .offset(x: -13, y: -9)
            }
        }
        .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .onTapGesture(perform: onSelect)
        .animation(.easeOut(duration: 0.22), value: selected)
    }

    private var cardBackground: some ShapeStyle {
        selected
            ? AnyShapeStyle(LinearGradient(
                colors: [DT.logoTint.opacity(0.15), DT.cardStrong],
                startPoint: .top,
                endPoint: .bottom
            ))
            : AnyShapeStyle(DT.cardMedium)
    }
}

private enum FlowTone {
    case neutral, blue, green, amber

    var background: Color {
        switch self {
        case .neutral: DT.cardStrong
        case .blue: DT.blueBg
        case .green: DT.greenBg
        case .amber: DT.amberBg
        }
    }

    var stroke: Color {
        switch self {
        case .neutral: DT.neutralBadgeStroke
        case .blue: DT.blueBadgeStroke
        case .green: DT.greenStroke
        case .amber: DT.amberStroke
        }
    }

    var text: Color {
        switch self {
        case .neutral: DT.textSecondary
        case .blue: DT.blueText
        case .green: DT.greenText
        case .amber: DT.amberText
        }
    }
}

private struct FlowToken: Identifiable {
    let id = UUID()
    let text: String
    let tone: FlowTone

    init(_ text: String, tone: FlowTone) {
        self.text = text
        self.tone = tone
    }
}

private struct TokenFlow: View {
    let tokens: [FlowToken]
    var compact = false

    var body: some View {
        HStack(spacing: compact ? 4 : 7) {
            ForEach(Array(tokens.enumerated()), id: \.element.id) { index, token in
                Text(token.text)
                    .font(.system(size: compact ? 9.2 : 11, weight: .bold))
                    .foregroundStyle(token.tone.text)
                    .padding(.horizontal, compact ? 7 : 11)
                    .padding(.vertical, compact ? 4 : 6)
                    .background(token.tone.background, in: RoundedRectangle(cornerRadius: compact ? 8 : 10, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: compact ? 8 : 10, style: .continuous)
                            .strokeBorder(token.tone.stroke, lineWidth: 1)
                    )
                    .fixedSize()
                if index < tokens.count - 1 {
                    Image(systemName: "arrow.right")
                        .font(.system(size: compact ? 7 : 9, weight: .semibold))
                        .foregroundStyle(DT.textFaint)
                }
            }
        }
    }
}

private struct FlowTrack: View {
    var body: some View {
        ZStack(alignment: .leading) {
            Capsule().fill(DT.neutralChipBg)
            LinearGradient(
                colors: [.clear, DT.logoTint, .clear],
                startPoint: .leading,
                endPoint: .trailing
            )
            .frame(width: 74)
            .offset(x: 32)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 3)
        .clipShape(Capsule())
    }
}

private struct SecondsPicker: View {
    let selected: Int
    let onSelect: (Int) -> Void

    var body: some View {
        HStack(spacing: 5) {
            ForEach([5, 10, 30], id: \.self) { seconds in
                Button("\(seconds) 秒") {
                    onSelect(seconds)
                }
                .buttonStyle(SecondsButtonStyle(selected: selected == seconds))
            }
        }
    }
}

private struct SecondsButtonStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 10.5, weight: .bold))
            .foregroundStyle(selected ? Color.white : DT.textSecondary)
            .padding(.horizontal, 11)
            .padding(.vertical, 5)
            .background(selected ? AnyShapeStyle(DT.primaryGradient) : AnyShapeStyle(DT.cardMedium), in: Capsule())
            .overlay(Capsule().strokeBorder(selected ? DT.blueBadgeStroke : DT.neutralBadgeStroke, lineWidth: 1))
            .brightness(configuration.isPressed ? -0.06 : 0)
    }
}

private struct SettingsInsetRow<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        content
            .padding(.horizontal, 15)
            .padding(.vertical, 13)
            .frame(maxWidth: .infinity, alignment: .leading)
            .sheetCard(
                fill: DT.cardMedium,
                stroke: DT.hairline,
                radius: 15,
                shadow: DT.softShadow,
                shadowRadius: 3,
                shadowY: 1
            )
    }
}

// MARK: - Mouse acceptance explanation

private struct MouseAcceptanceExplanation: View {
    let waitSeconds: Int

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            AcceptanceRule(
                icon: "checkmark",
                text: "鼠标进入调度工作桌面 → 视为已接收，停止倒计时",
                tone: .green
            )
            AcceptanceRule(
                icon: "arrow.uturn.backward",
                text: "鼠标仍在其他工作桌面 → 倒计时结束后恢复 ActRealm 工作区",
                tone: .amber
            )

            VStack(spacing: 12) {
                HStack(spacing: 12) {
                    DesktopDiagram(
                        title: "调度工作桌面",
                        labels: ["ActRealm 工作区", "Agent"],
                        selected: true,
                        showsMouse: false
                    )
                    Image(systemName: "arrow.left")
                        .font(.system(size: 15, weight: .bold))
                        .foregroundStyle(DT.logoTint)
                    DesktopDiagram(
                        title: "其他工作桌面",
                        labels: ["Browser", "Mail"],
                        selected: false,
                        showsMouse: true
                    )
                }

                HStack(spacing: 9) {
                    ZStack {
                        ConicRing(
                            fraction: 0.58,
                            color: DT.amberDot,
                            size: 28,
                            lineWidth: 3,
                            coreBackground: DT.cardStrong
                        )
                        Text("\(waitSeconds)s")
                            .font(.system(size: 8, weight: .heavy))
                            .foregroundStyle(DT.amberText)
                    }
                    Text("当前未接收：鼠标仍在其他工作桌面 · 倒计时结束后返回 ActRealm 工作区")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(DT.amberText)
                    Spacer()
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 9)
                .background(DT.amberBg.opacity(0.75), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 11, style: .continuous).strokeBorder(DT.amberStroke.opacity(0.75), lineWidth: 1))
            }
            .padding(15)
            .background(DT.cardFaint, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(DT.hairlineSoft, lineWidth: 1))
        }
    }
}

private struct AcceptanceRule: View {
    let icon: String
    let text: String
    let tone: FlowTone

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: icon)
                .font(.system(size: 9, weight: .heavy))
                .foregroundStyle(tone.text)
                .frame(width: 18, height: 18)
                .background(tone.background, in: Circle())
            Text(text)
                .font(DT.body(11))
                .foregroundStyle(DT.textSecondary)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tone.background.opacity(0.68), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 11, style: .continuous).strokeBorder(tone.stroke.opacity(0.75), lineWidth: 1))
    }
}

private struct DesktopDiagram: View {
    let title: String
    let labels: [String]
    let selected: Bool
    let showsMouse: Bool

    var body: some View {
        ZStack(alignment: .bottomTrailing) {
            VStack(alignment: .leading, spacing: 9) {
                Text(title)
                    .font(.system(size: 9.5, weight: .heavy))
                    .kerning(0.3)
                    .foregroundStyle(selected ? DT.blueText : DT.textSecondary)
                HStack(spacing: 7) {
                    ForEach(labels, id: \.self) { label in
                        Text(label)
                            .font(.system(size: 10, weight: label == "ActRealm 工作区" ? .bold : .semibold))
                            .foregroundStyle(label == "ActRealm 工作区" ? DT.blueText : DT.textSecondary)
                            .padding(.horizontal, 9)
                            .padding(.vertical, 5)
                            .background(DT.cardStrong, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 8, style: .continuous)
                                    .strokeBorder(label == "ActRealm 工作区" ? DT.blueBadgeStroke : DT.neutralBadgeStroke, lineWidth: 1)
                            )
                    }
                }
            }
            .frame(maxWidth: .infinity, minHeight: 66, alignment: .topLeading)

            if showsMouse {
                HStack(spacing: 5) {
                    StatusDot(color: DT.amberDot, size: 10, glow: true)
                    Text("鼠标")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundStyle(DT.amberText)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 11)
        .background(selected ? DT.logoTint.opacity(0.08) : DT.cardMedium, in: RoundedRectangle(cornerRadius: 13, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 13, style: .continuous)
                .strokeBorder(selected ? DT.logoTint.opacity(0.55) : DT.neutralBadgeStroke, lineWidth: 1.5)
        )
        .frame(maxWidth: .infinity)
    }
}

// MARK: - Live rule preview

private struct RulePreviewContent: View {
    let settings: ForegroundSchedulingSettings

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 7) {
                ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                    PreviewStep(step: step)
                    if index < steps.count - 1 {
                        Image(systemName: "arrow.right")
                            .font(.system(size: 9, weight: .semibold))
                            .foregroundStyle(DT.textFaint)
                            .padding(.top, 9)
                    }
                }
                Spacer(minLength: 0)
            }

            FlowTrack()

            HStack(spacing: 8) {
                StatusDot(color: DT.logoTint, size: 6)
                Text(summary)
                    .font(DT.body(11.5))
                    .foregroundStyle(DT.textSecondary)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .background(DT.cardFaint, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(DT.hairline, lineWidth: 1))
    }

    private var steps: [PreviewStepModel] {
        guard settings.isEnabled else {
            return [
                .init("新任务", tone: .neutral),
                .init("进入待处理列表", tone: .blue, branch: "手动切换窗口处理", branchTone: .neutral),
            ]
        }
        switch settings.strategy {
        case .immediate:
            return [
                .init("新任务", tone: .neutral),
                .init("打开 Agent", tone: .blue),
                .init(
                    "等待桌面接收 \(settings.acceptanceSeconds) 秒",
                    tone: .green,
                    branch: settings.returnsToActRealmWorkspace ? "未进入桌面 → 返回 ActRealm 工作区" : "保持前台继续等待",
                    branchTone: settings.returnsToActRealmWorkspace ? .amber : .green
                ),
            ]
        case .remind:
            return [
                .init("新任务", tone: .neutral),
                .init("提醒 \(settings.reminderSeconds) 秒", tone: .amber, branch: "期间已处理 → 不再打开", branchTone: .amber),
                .init("打开 Agent", tone: .blue),
                .init(
                    "等待桌面接收 \(settings.acceptanceSeconds) 秒",
                    tone: .green,
                    branch: settings.returnsToActRealmWorkspace ? "未进入桌面 → 返回 ActRealm 工作区" : "保持前台继续等待",
                    branchTone: settings.returnsToActRealmWorkspace ? .amber : .green
                ),
            ]
        case .actRealmWorkspace:
            return [
                .init("新任务", tone: .neutral),
                .init("进入待处理列表", tone: .blue),
                .init("手动打开", tone: .green, branch: "手动打开，无需接收倒计时", branchTone: .neutral),
            ]
        }
    }

    private var summary: String {
        guard settings.isEnabled else {
            return "台前调度已关闭：新任务只进入待处理列表，需手动切换窗口"
        }
        switch settings.strategy {
        case .immediate:
            let ending = settings.returnsToActRealmWorkspace
                ? "，\(settings.acceptanceSeconds) 秒内未进入桌面则返回 ActRealm 工作区"
                : "，不自动返回"
            return "新任务直接在调度桌面打开对应 Agent\(ending)"
        case .remind:
            let ending = settings.returnsToActRealmWorkspace
                ? "；\(settings.acceptanceSeconds) 秒未进入桌面则返回"
                : ""
            return "先在 ActRealm 工作区提醒 \(settings.reminderSeconds) 秒，未处理再自动打开 Agent\(ending)"
        case .actRealmWorkspace:
            return "任务只进入待处理列表，不自动切换窗口，需手动打开"
        }
    }
}

private struct PreviewStepModel {
    let label: String
    let tone: FlowTone
    let branch: String?
    let branchTone: FlowTone

    init(_ label: String, tone: FlowTone, branch: String? = nil, branchTone: FlowTone = .neutral) {
        self.label = label
        self.tone = tone
        self.branch = branch
        self.branchTone = branchTone
    }
}

private struct PreviewStep: View {
    let step: PreviewStepModel

    var body: some View {
        VStack(spacing: 5) {
            Text(step.label)
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(step.tone.text)
                .padding(.horizontal, 11)
                .padding(.vertical, 6)
                .background(step.tone.background, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(step.tone.stroke, lineWidth: 1))
                .fixedSize()
            if let branch = step.branch {
                Image(systemName: "arrow.down")
                    .font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(DT.textFaint)
                Text(branch)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(step.branchTone.text)
                    .padding(.horizontal, 9)
                    .padding(.vertical, 4)
                    .background(step.branchTone.background, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(step.branchTone.stroke, lineWidth: 1))
                    .fixedSize()
            }
        }
    }
}

// MARK: - Provider override rows

private struct ProviderRuleRow: View {
    let provider: ProviderKind
    let name: String
    let globalStrategy: ForegroundArrivalStrategy
    let selected: ForegroundAgentRule
    let onSelect: (ForegroundAgentRule) -> Void

    var body: some View {
        HStack(spacing: 12) {
            ProviderAvatar(kind: provider, size: 26)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                Text(subtitle)
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            HStack(spacing: 6) {
                ForEach(ForegroundAgentRule.allCases, id: \.self) { rule in
                    Button(rule.shortTitle) {
                        onSelect(rule)
                    }
                    .buttonStyle(RuleButtonStyle(selected: selected == rule))
                }
            }
        }
        .padding(.horizontal, 15)
        .padding(.vertical, 13)
        .sheetCard(
            fill: DT.cardMedium,
            stroke: DT.hairline,
            radius: 15,
            shadow: DT.softShadow,
            shadowRadius: 3,
            shadowY: 1
        )
    }

    private var subtitle: String {
        if selected == .defaultRule {
            return "默认跟随全局 · \(globalStrategy.shortTitle)"
        }
        return selected.fullTitle
    }
}

private struct RuleButtonStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 10.5, weight: .bold))
            .foregroundStyle(selected ? Color.white : DT.textSecondary)
            .padding(.horizontal, 12)
            .padding(.vertical, 5)
            .background(selected ? AnyShapeStyle(DT.primaryGradient) : AnyShapeStyle(DT.cardMedium), in: Capsule())
            .overlay(Capsule().strokeBorder(selected ? DT.blueBadgeStroke : DT.neutralBadgeStroke, lineWidth: 1))
            .brightness(configuration.isPressed ? -0.06 : 0)
    }
}

private extension ForegroundArrivalStrategy {
    var shortTitle: String {
        switch self {
        case .immediate: "立即打开"
        case .remind: "先提醒，再打开"
        case .actRealmWorkspace: "只留在 ActRealm 工作区"
        }
    }
}

private extension ForegroundAgentRule {
    var shortTitle: String {
        switch self {
        case .defaultRule: "使用默认"
        case .immediate: "立即打开"
        case .remind: "先提醒"
        case .actRealmWorkspace: "只进待处理"
        }
    }

    var fullTitle: String {
        switch self {
        case .defaultRule: "跟随全局策略"
        case .immediate: "立即打开 Agent"
        case .remind: "先提醒，再打开"
        case .actRealmWorkspace: "只进入待处理列表"
        }
    }
}
