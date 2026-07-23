import ActRealmKit
import SwiftUI

/// Native management screen for 智能聚焦（Agent Focus）. Every edit is
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
                eventTriggers
                workspaceStatus
                arrivalStrategies
                stageManagerBehavior
                afterOpening
            }
            .opacity(settings.isEnabled ? 1 : 0.45)
            .allowsHitTesting(settings.isEnabled)

            rulePreview
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
                Text("智能聚焦")
                    .font(.system(size: 23, weight: .heavy))
                    .foregroundStyle(DT.textStrong)
                Text("Agent Focus · 在需要你判断时提醒并带回对应 Agent")
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
        if !settings.isEnabled { return "当前：智能聚焦已关闭" }
        if settings.workspaceApps.isEmpty { return "当前：待绑定桌面" }
        if model.foregroundQueuedCount > 0 { return "当前：队列中 (model.foregroundQueuedCount) 项" }
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
                    title: "启用智能聚焦",
                    detail: "关闭后事件仍进入 ActRealm，但不显示聚焦倒计时，也不自动切换 Agent。",
                    isOn: binding(\.isEnabled)
                )
            }
        }
    }

    private var eventTriggers: some View {
        SchedulingPageCard {
            VStack(alignment: .leading, spacing: 13) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("触发智能聚焦的事件")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("关闭只影响聚焦，事件仍正常进入 ActRealm")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                }

                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 9) {
                    AgentFocusEventToggle(
                        title: "等待批准",
                        detail: "请求运行命令或执行操作",
                        isOn: eventRuleBinding(\.approval)
                    )
                    AgentFocusEventToggle(
                        title: "Agent 提问",
                        detail: "需要用户回答的问题",
                        isOn: eventRuleBinding(\.question)
                    )
                    AgentFocusEventToggle(
                        title: "出错或卡住",
                        detail: "任务失败或需要检查",
                        isOn: eventRuleBinding(\.error)
                    )
                    AgentFocusEventToggle(
                        title: "任务完成",
                        detail: "本轮完成、等待确认",
                        isOn: eventRuleBinding(\.completion)
                    )
                }
            }
        }
    }

    // MARK: - Workspace status

    private var workspaceStatus: some View {
        let workspace = model.foregroundWorkspaceStatus
        return SchedulingPageCard {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 10) {
                    Text("Agent 绑定工作区")
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
                            Text("先选择 Agent 绑定工作区")
                                .font(.system(size: 13, weight: .bold))
                                .foregroundStyle(DT.textPrimary)
                            Text("切换到放置 Claude、Codex 或终端的工作区后，绑定当前显示器；只有已绑定 Agent 才会触发聚焦。")
                                .font(DT.body(11))
                                .foregroundStyle(DT.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer()
                        Button("选择绑定工作区…") {
                            model.beginForegroundWorkspaceSelection()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                } else {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack(spacing: 8) {
                            if let displayName = settings.workspaceDisplayName {
                                Label(displayName, systemImage: "display")
                                    .font(.system(size: 10.5, weight: .bold))
                                    .foregroundStyle(DT.blueText)
                                    .padding(.horizontal, 9)
                                    .padding(.vertical, 4)
                                    .background(DT.blueBg, in: Capsule())
                                    .overlay(Capsule().strokeBorder(DT.blueBadgeStroke, lineWidth: 1))
                            }
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
                                workspace.isPointerInsideSchedulingWorkspace
                                    ? "鼠标已进入绑定工作区"
                                    : workspace.isSchedulingWorkspaceActive
                                        ? "绑定工作区可见，等待鼠标进入"
                                        : "当前位于其他工作区",
                                systemImage: workspace.isPointerInsideSchedulingWorkspace
                                    ? "cursorarrow.motionlines.click"
                                    : "circle.dashed"
                            )
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(workspace.isPointerInsideSchedulingWorkspace ? DT.greenText : DT.textWeak)
                            Spacer()
                            Button("测试智能聚焦") { model.testForegroundScheduling() }
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
                    Text("聚焦方式")
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
                        title: "立即聚焦",
                        detail: "收到允许触发的事件后，直接打开对应 Agent 的具体任务。",
                        steps: [
                            FlowToken("事件到达", tone: .neutral),
                            FlowToken("具体任务", tone: .blue),
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
                        title: "提醒后聚焦",
                        detail: "先显示 HUD；可立即查看、稍后处理，或在倒计时后自动打开。",
                        steps: [
                            FlowToken("事件到达", tone: .neutral),
                            FlowToken("HUD 倒计时", tone: .amber),
                            FlowToken("具体任务", tone: .blue),
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
                        title: "仅进入 ActRealm",
                        detail: "只保存事件，不显示聚焦倒计时，也不自动切换页面。",
                        steps: [
                            FlowToken("事件到达", tone: .neutral),
                            FlowToken("ActRealm", tone: .blue),
                            FlowToken("手动查看", tone: .green),
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

    // MARK: - macOS Stage Manager

    private var stageManagerBehavior: some View {
        let available = settings.strategy != .actRealmWorkspace
        return SchedulingPageCard {
            VStack(alignment: .leading, spacing: 13) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("聚焦时使用 macOS 台前调度")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("只恢复本次智能聚焦主动改变的状态")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                }

                SettingsInsetRow {
                    HStack(spacing: 14) {
                        VStack(alignment: .leading, spacing: 3) {
                            Text("允许智能聚焦启用台前调度")
                                .font(.system(size: 12.5, weight: .bold))
                                .foregroundStyle(DT.textPrimary)
                            Text("先记录进入前状态；原本开启时保持开启，原本关闭时才临时开启。")
                                .font(DT.body(10.5))
                                .foregroundStyle(DT.textWeak)
                        }
                        Spacer()
                        SchedulingSwitch(isOn: binding(\.allowsStageManager))
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("恢复进入前状态的时机")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(DT.textSecondary)
                    HStack(spacing: 8) {
                        RestoreTimingButton(
                            title: "用户接收后恢复",
                            selected: settings.stageManagerRestoreTiming == .afterAcceptance
                        ) {
                            model.updateForegroundScheduling {
                                $0.stageManagerRestoreTiming = .afterAcceptance
                            }
                        }
                        RestoreTimingButton(
                            title: "返回 ActRealm 时恢复",
                            selected: settings.stageManagerRestoreTiming == .onReturnToActRealm,
                            recommended: true
                        ) {
                            model.updateForegroundScheduling {
                                $0.stageManagerRestoreTiming = .onReturnToActRealm
                            }
                        }
                        RestoreTimingButton(
                            title: "保持开启",
                            selected: settings.stageManagerRestoreTiming == .keepEnabled
                        ) {
                            model.updateForegroundScheduling {
                                $0.stageManagerRestoreTiming = .keepEnabled
                            }
                        }
                    }
                }
                .opacity(settings.allowsStageManager ? 1 : 0.4)
                .allowsHitTesting(settings.allowsStageManager)
            }
            .opacity(available ? 1 : 0.5)
            .allowsHitTesting(available)
        }
    }

    // MARK: - After opening

    private var afterOpening: some View {
        let available = settings.strategy != .actRealmWorkspace
        return SchedulingPageCard {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text("未接收时自动返回")
                        .font(.system(size: 14, weight: .heavy))
                        .foregroundStyle(DT.textPrimary)
                    Text("鼠标进入绑定工作区即视为已接收，不要求点击或键盘输入")
                        .font(DT.body(11.5))
                        .foregroundStyle(DT.textSecondary)
                    Spacer()
                    if !available {
                        Text("「仅进入 ActRealm」无需此行为")
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
                                Text("开启自动返回")
                                    .font(.system(size: 12.5, weight: .bold))
                                    .foregroundStyle(DT.textPrimary)
                                Text("超时未检测到鼠标进入绑定工作区时，返回 ActRealm；事件仍保留。")
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
                                Text("接收等待时间")
                                    .font(.system(size: 12.5, weight: .bold))
                                    .foregroundStyle(DT.textPrimary)
                                Text("从 Agent 打开后开始计算，可选 5 / 10 / 30 秒。")
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

    private func chooseStrategy(_ strategy: ForegroundArrivalStrategy) {
        model.updateForegroundScheduling { $0.strategy = strategy }
        let label: String
        switch strategy {
        case .immediate: label = "立即聚焦"
        case .remind: label = "提醒后聚焦"
        case .actRealmWorkspace: label = "仅进入 ActRealm"
        }
        model.showToast("聚焦方式：\(label)")
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

    private func eventRuleBinding(
        _ keyPath: WritableKeyPath<AgentFocusEventRules, Bool>
    ) -> Binding<Bool> {
        Binding(
            get: { model.foregroundScheduling.eventRules[keyPath: keyPath] },
            set: { value in
                model.updateForegroundScheduling {
                    $0.eventRules[keyPath: keyPath] = value
                }
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

private struct AgentFocusEventToggle: View {
    let title: String
    let detail: String
    @Binding var isOn: Bool

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                Text(detail)
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            SchedulingSwitch(isOn: $isOn)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
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

private struct RestoreTimingButton: View {
    let title: String
    let selected: Bool
    var recommended = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                Text(title)
                if recommended {
                    Text("推荐")
                        .font(.system(size: 8.5, weight: .heavy))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(selected ? Color.white.opacity(0.16) : DT.blueBg, in: Capsule())
                }
            }
            .font(.system(size: 10.5, weight: .bold))
            .foregroundStyle(selected ? Color.white : DT.textSecondary)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(
                selected ? AnyShapeStyle(DT.primaryGradient) : AnyShapeStyle(DT.cardMedium),
                in: RoundedRectangle(cornerRadius: 11, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 11, style: .continuous)
                    .strokeBorder(selected ? DT.blueBadgeStroke : DT.neutralBadgeStroke, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
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
        return "智能聚焦已就绪"
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
    @EnvironmentObject private var model: AppModel
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.snapshotRendering) private var snapshotRendering

    var body: some View {
        TimelineView(.animation(
            minimumInterval: 1 / 15,
            paused: reduceMotion || snapshotRendering || !model.isWorkspaceAnimationActive
        )) { timeline in
            GeometryReader { proxy in
                let glowWidth = min(84, max(54, proxy.size.width * 0.24))
                let phase = flowPhase(at: timeline.date)

                ZStack(alignment: .leading) {
                    Capsule().fill(DT.neutralChipBg)
                    LinearGradient(
                        colors: [.clear, DT.logoTint.opacity(0.45), DT.logoTint, .clear],
                        startPoint: .leading,
                        endPoint: .trailing
                    )
                    .frame(width: glowWidth)
                    .offset(
                        x: -glowWidth
                            + (proxy.size.width + glowWidth) * phase
                    )
                }
            }
        }
        .frame(maxWidth: .infinity)
        .frame(height: 3)
        .clipShape(Capsule())
    }

    private func flowPhase(at date: Date) -> CGFloat {
        if reduceMotion || snapshotRendering || !model.isWorkspaceAnimationActive { return 0.42 }
        let duration = 1.75
        return CGFloat(
            date.timeIntervalSinceReferenceDate
                .truncatingRemainder(dividingBy: duration) / duration
        )
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
                text: "鼠标进入 Agent 绑定工作区 → 视为已接收，停止自动返回",
                tone: .green
            )
            AcceptanceRule(
                icon: "arrow.uturn.backward",
                text: "鼠标仍在其他工作区 → 倒计时结束后按设置返回 ActRealm",
                tone: .amber
            )

            VStack(spacing: 12) {
                HStack(spacing: 12) {
                    DesktopDiagram(
                        title: "Agent 绑定工作区",
                        labels: ["ActRealm 工作区", "Agent"],
                        selected: true,
                        showsMouse: false
                    )
                    Image(systemName: "arrow.left")
                        .font(.system(size: 15, weight: .bold))
                        .foregroundStyle(DT.logoTint)
                    DesktopDiagram(
                        title: "其他工作区",
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
                    Text("当前未接收：鼠标仍在其他工作区 · 倒计时结束后返回 ActRealm")
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
                .init("Agent 事件", tone: .neutral),
                .init("进入 ActRealm", tone: .blue, branch: "不显示聚焦 HUD · 不切换", branchTone: .neutral),
            ]
        }
        switch settings.strategy {
        case .immediate:
            return [
                .init("允许触发的事件", tone: .neutral),
                .init(settings.allowsStageManager ? "记录系统状态" : "保持系统状态", tone: .amber),
                .init("聚焦具体任务", tone: .blue),
                .init(
                    "等待鼠标进入 \(settings.acceptanceSeconds) 秒",
                    tone: .green,
                    branch: settings.returnsToActRealmWorkspace ? "未接收 → 返回 ActRealm" : "未接收 → 保持 Agent 页面",
                    branchTone: settings.returnsToActRealmWorkspace ? .amber : .green
                ),
            ]
        case .remind:
            return [
                .init("允许触发的事件", tone: .neutral),
                .init("HUD \(settings.reminderSeconds) 秒", tone: .amber, branch: "已解决或稍后处理 → 取消", branchTone: .amber),
                .init("聚焦具体任务", tone: .blue),
                .init(
                    "等待鼠标进入 \(settings.acceptanceSeconds) 秒",
                    tone: .green,
                    branch: settings.returnsToActRealmWorkspace ? "未接收 → 返回 ActRealm" : "未接收 → 保持 Agent 页面",
                    branchTone: settings.returnsToActRealmWorkspace ? .amber : .green
                ),
            ]
        case .actRealmWorkspace:
            return [
                .init("允许触发的事件", tone: .neutral),
                .init("进入 ActRealm", tone: .blue),
                .init("等待手动查看", tone: .green, branch: "不自动切换页面", branchTone: .neutral),
            ]
        }
    }

    private var summary: String {
        guard settings.isEnabled else {
            return "智能聚焦已关闭：事件照常进入 ActRealm，不显示聚焦倒计时，也不切换页面"
        }
        switch settings.strategy {
        case .immediate:
            let ending = settings.returnsToActRealmWorkspace
                ? "，\(settings.acceptanceSeconds) 秒内鼠标未进入绑定工作区则返回 ActRealm"
                : "，不自动返回"
            return "允许触发的事件会立即聚焦对应 Agent 的具体任务\(ending)"
        case .remind:
            let ending = settings.returnsToActRealmWorkspace
                ? "；\(settings.acceptanceSeconds) 秒未接收则返回 ActRealm"
                : ""
            return "先显示 \(settings.reminderSeconds) 秒 HUD，可立即查看或稍后处理；倒计时后聚焦 Agent\(ending)"
        case .actRealmWorkspace:
            return "事件只进入 ActRealm，不显示聚焦倒计时，也不自动切换页面"
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
