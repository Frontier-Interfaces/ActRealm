import ActRealmKit
import SwiftUI

struct AgentTasksSection: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    let onOpenSetup: () -> Void

    private var tasks: [LaneTask] {
        let cutoff = model.now.addingTimeInterval(-30 * 60)
        return model.derived.agentTasks.filter { task in
            guard !model.isTaskDismissed(task) else { return false }
            return task.status == .running || task.status == .waiting
                || task.hasVisibleAttention || task.lastEventAt >= cutoff
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("AGENT TASKS")
                    .font(.system(size: 13, weight: .heavy))
                    .kerning(0.65)
                    .foregroundStyle(DT.textPrimary)
                Text("正在进行的任务 · 点击展开详情")
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textSecondary)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(summary)
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
            }

            if tasks.isEmpty {
                if model.setupInfo == nil {
                    SetupDetectionState()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if model.isFirstRun {
                    FirstRunTasksEmpty(onOpenSetup: onOpenSetup)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Text("没有正在进行的任务")
                        .font(.system(size: 11))
                        .foregroundStyle(DT.textWeak)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            } else if snapshotRendering {
                taskList
            } else {
                ScrollView(.vertical, showsIndicators: true) { taskList }
                    .scrollBounceBehavior(.basedOnSize)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .liquidGlassSurface(
            tint: DT.cardSoft.opacity(0.18),
            radius: 24,
            stroke: DT.hairline,
            shadow: DT.cardShadow.opacity(0.8),
            shadowRadius: 30,
            shadowY: 12
        )
    }

    private var taskList: some View {
        LazyVStack(spacing: 6) {
            ForEach(tasks) { task in
                TaskRow(task: task, expanded: model.expandedTaskId == task.id)
                    .id(task.id)
            }
        }
        .padding(.top, 11)
    }

    private var summary: String {
        let waiting = tasks.filter { $0.status == .waiting }.count
        let running = tasks.filter { $0.status == .running }.count
        let completed = tasks.filter { $0.status == .done }.count
        return "\(tasks.count) 个任务 · \(waiting) 等待 · \(running) 运行中 · \(completed) 已完成"
    }
}

private struct SetupDetectionState: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 9) {
            if model.bridgeStatus.isListening {
                ProgressView().controlSize(.small)
            } else {
                Image(systemName: "bolt.horizontal.circle")
                    .font(.system(size: 25, weight: .light))
                    .foregroundStyle(DT.textWeak)
            }
            Text(model.bridgeStatus.isListening ? "正在检测本机 Agent" : "正在等待 Runtime")
                .font(.system(size: 11.5, weight: .semibold))
                .foregroundStyle(DT.textSecondary)
            Text("接入状态确认前不会显示伪造的任务或额度")
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textWeak)
        }
    }
}

private struct FirstRunTasksEmpty: View {
    let onOpenSetup: () -> Void

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "point.3.connected.trianglepath.dotted")
                .font(.system(size: 30, weight: .light))
                .foregroundStyle(DT.logoTint)
            Text("尚未连接任何 Agent")
                .font(.system(size: 14, weight: .bold))
                .foregroundStyle(DT.textStrong)
            Text("连接 Claude 或 Codex 后，运行中的任务与待处理事项会显示在这里。数据仅留在本机。")
                .font(.system(size: 10.5))
                .foregroundStyle(DT.textWeak)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 300)
            Button("＋ 连接 Agent", action: onOpenSetup)
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11, horizontalPadding: 15))
        }
    }
}

private struct TaskRow: View {
    @EnvironmentObject private var model: AppModel
    let task: LaneTask
    let expanded: Bool

    private var provider: ProviderKind { ProviderKind(record: task.session.provider) ?? .codex }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 7) {
                ProviderAvatar(kind: provider, size: 20)
                Text(fieldVisible("task") ? task.title : providerName)
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(titleColor)
                    .lineLimit(1)
                Chip(text: badge, tone: .forStatus(task.status), fontSize: 9.5)
                Spacer(minLength: 5)
                if fieldVisible("activity") {
                    Text(rightStatus)
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(rightColor)
                        .lineLimit(1)
                }
            }

            if let prompt = promptPreview, !prompt.isEmpty {
                HStack(spacing: 6) {
                    Text("PROMPT")
                        .font(.system(size: 8.5, weight: .heavy))
                        .kerning(0.5)
                        .foregroundStyle(DT.textWeak)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 5))
                        .overlay(RoundedRectangle(cornerRadius: 5).strokeBorder(DT.neutralChipStroke, lineWidth: 1))
                    Text(prompt)
                        .font(.system(size: 11.5, weight: .medium))
                        .foregroundStyle(DT.textSecondary)
                        .lineLimit(1)
                }
                .padding(.top, 5)
                .padding(.bottom, 2)
            }

            usageStrip

            HStack(spacing: 7) {
                if fieldVisible("model") || fieldVisible("project") {
                    Text(metaLine)
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
                }
                if fieldVisible("plan"), let plan = task.planProgress {
                    Text("计划 \(plan.done)/\(plan.total)")
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                    ProgressTrack(fraction: Double(plan.done) / Double(plan.total))
                        .frame(width: 70, height: 4)
                    if let activity = task.activity, activity.contains("子 Agent") {
                        Text(activity)
                            .font(.system(size: 9.5))
                            .foregroundStyle(DT.textWeak)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 4)
                Button("清除") { model.dismissTask(task) }
                    .buttonStyle(ClearTaskButtonStyle())
                    .help(task.openOutboxCount > 0
                        ? "清除任务并安全交还 \(task.openOutboxCount) 项待处理事项"
                        : "从列表中移除该任务")
            }
            .padding(.top, 3)

            if expanded {
                expandedDetails
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(.horizontal, 13)
        .padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(rowBackground, in: RoundedRectangle(cornerRadius: 15, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 15, style: .continuous)
                .strokeBorder(borderColor, lineWidth: 1)
        )
        .shadow(color: task.status == .waiting ? DT.cardShadow.opacity(0.35) : .clear, radius: 9, y: 3)
        .contentShape(RoundedRectangle(cornerRadius: 15, style: .continuous))
        .onTapGesture {
            withAnimation(.easeOut(duration: 0.25)) {
                model.expandedTaskId = expanded ? nil : task.id
                model.pinnedSessionId = expanded ? nil : task.id
            }
        }
    }

    private var expandedDetails: some View {
        VStack(alignment: .leading, spacing: 9) {
            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 6) {
                ForEach(Array(detailItems.enumerated()), id: \.offset) { pair in
                    DetailLine(
                        label: pair.element.label,
                        value: pair.element.value,
                        emphasized: pair.element.emphasized
                    )
                }
            }
            .font(.system(size: 11))

            HStack(spacing: 10) {
                if task.openOutboxCount > 0 {
                    Button("查看待处理事项") {
                        withAnimation(.easeOut(duration: 0.2)) { model.revealOutbox(for: task) }
                    }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                }
                if fieldVisible("jump"),
                   task.session.jumpCapability != nil,
                   task.session.jumpCapability != "unsupported" {
                    Button("打开应用") {
                        Task { await model.jump(to: task) }
                    }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                }
                if fieldVisible("control"), task.session.canManage == true {
                    Button("连接托管") {
                        Task { await model.manage(task) }
                    }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                }
                Text(note)
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textFaint)
                    .lineLimit(1)
            }
        }
        .padding(.top, 10)
        .overlay(alignment: .top) {
            Rectangle().fill(DT.separator).frame(height: 1)
        }
        .padding(.top, 10)
    }

    @ViewBuilder
    private var usageStrip: some View {
        if (fieldVisible("tokens") && (task.totalTokens != nil || task.estimatedCostUsdMicros != nil))
            || (fieldVisible("context") && task.contextUsageFraction != nil) {
            HStack(spacing: 6) {
                if fieldVisible("tokens"), let total = task.totalTokens {
                    usageChip("累计 \(ZhFormat.tokenCount(total)) Token", tone: .neutral)
                }
                if fieldVisible("context"), let fraction = task.contextUsageFraction {
                    usageChip("上下文 \(Int((fraction * 100).rounded()))%", tone: fraction >= 0.7 ? .amber : .blue)
                }
                if fieldVisible("tokens"), task.estimatedCostUsdMicros != nil {
                    usageChip("估算 API 价格 \(estimatedCostText)", tone: .blue)
                }
                Spacer(minLength: 0)
            }
            .padding(.top, 5)
        }
    }

    private func usageChip(_ text: String, tone: Chip.Tone) -> some View {
        Chip(text: text, tone: tone, fontSize: 8.5)
    }

    private var promptPreview: String? {
        guard fieldVisible("task") else { return nil }
        guard let providerTitle = task.session.providerTitle,
              let taskTitle = task.session.title,
              providerTitle != taskTitle
        else { return nil }
        return taskTitle
    }
    private var workspace: String {
        if let environment = task.session.environment, !environment.isEmpty { return environment }
        switch provider {
        case .claude: return "Claude Code CLI"
        case .codex: return "Codex CLI"
        case .gemini: return "Gemini CLI"
        }
    }
    private var contextText: String {
        if let used = task.contextUsedTokens, let window = task.contextWindowTokens {
            let percent = task.contextUsageFraction.map { Int(($0 * 100).rounded()) }
            return "\(ZhFormat.tokenCount(used)) / \(ZhFormat.tokenCount(window))\(percent.map { " · \($0)%" } ?? "")"
        }
        if let fraction = task.contextUsageFraction { return "\(Int((fraction * 100).rounded()))%" }
        return "暂无数据"
    }
    private var contextIsTight: Bool { (task.contextUsageFraction ?? 0) >= 0.7 }
    private var planText: String {
        if let plan = task.planProgress { return "\(plan.done)/\(plan.total)（进行中）" }
        return provider == .codex ? "未提供计划事件" : "未提供"
    }
    private var tokenText: String { task.totalTokens.map(ZhFormat.tokenCount) ?? "暂无数据" }
    private var tokenIsHigh: Bool { (task.totalTokens ?? 0) >= 140_000 }
    private var lastTurnTokenText: String { task.lastTurnTokens.map(ZhFormat.tokenCount) ?? "—" }
    private var estimatedCostText: String {
        guard let micros = task.estimatedCostUsdMicros else { return "—" }
        let dollars = Double(micros) / 1_000_000
        return dollars > 0 && dollars < 0.01
            ? String(format: "$%.4f", dollars)
            : String(format: "$%.2f", dollars)
    }
    private var recoveryText: String {
        switch task.session.recoveryState {
        case "controllable": "已重新连接，可控制"
        case "observing": "仍在运行，仅可观察"
        case "waiting_for_event": "历史已恢复，等待新事件"
        case "lost_control": "已失去控制"
        case "ended": "已结束"
        default: "等待确认状态"
        }
    }
    private var controlText: String {
        task.session.controlCapability == "managed"
            ? "app-server 托管，可回答提问"
            : "外部 Hook，仅观察 / 授权"
    }
    private var metaLine: String {
        var parts = [providerName]
        if fieldVisible("project"), let project = task.projectName, !project.isEmpty { parts.append(project) }
        if fieldVisible("model") { parts.append(task.model ?? "模型未知") }
        return parts.joined(separator: " · ")
    }
    private func fieldVisible(_ field: String) -> Bool {
        model.uiSettings.taskCardFields.contains(field)
    }
    private var detailItems: [(label: String, value: String, emphasized: Bool)] {
        var items: [(String, String, Bool)] = []
        if fieldVisible("environment") { items.append(("工作区", workspace, false)) }
        if fieldVisible("context") { items.append(("本轮上下文", contextText, contextIsTight)) }
        if fieldVisible("plan") { items.append(("计划", planText, false)) }
        if fieldVisible("tokens") {
            items.append(("会话累计 Token", tokenText, tokenIsHigh))
            items.append(("本轮 Token", lastTurnTokenText, false))
            items.append(("估算 API 价格", estimatedCostText, false))
            if task.inputTokens != nil || task.outputTokens != nil {
                items.append((
                    "输入 / 输出",
                    "\(task.inputTokens.map(ZhFormat.tokenCount) ?? "—") / \(task.outputTokens.map(ZhFormat.tokenCount) ?? "—")",
                    false
                ))
            }
            if task.session.cacheReadTokens != nil || task.session.cacheCreationTokens != nil {
                items.append((
                    "缓存读取 / 写入",
                    "\(task.session.cacheReadTokens.map(ZhFormat.tokenCount) ?? "—") / \(task.session.cacheCreationTokens.map(ZhFormat.tokenCount) ?? "—")",
                    false
                ))
            }
            if let reasoning = task.session.reasoningTokens {
                items.append(("推理 Token", ZhFormat.tokenCount(reasoning), false))
            }
        }
        if fieldVisible("tool") { items.append(("当前工具", task.session.currentTool ?? "—", false)) }
        if fieldVisible("permissionMode") { items.append(("权限模式", task.session.permissionMode ?? "—", false)) }
        if fieldVisible("subagents") { items.append(("运行中的子 Agent", "\(task.session.activeSubagents ?? 0)", false)) }
        if fieldVisible("recovery") { items.append(("恢复状态", recoveryText, false)) }
        if fieldVisible("control") { items.append(("托管能力", controlText, false)) }
        if fieldVisible("jump") { items.append(("打开应用", task.session.jumpCapability == "unsupported" ? "当前环境不支持" : "可用", false)) }
        if fieldVisible("titleSource") { items.append(("标题来源", task.session.providerTitleSource ?? "—", false)) }
        if fieldVisible("sessionId") { items.append(("ActRealm Session ID", task.id, false)) }
        if fieldVisible("providerSessionId") { items.append(("Provider Session ID", task.session.providerSessionId, false)) }
        if fieldVisible("providerTurnId") { items.append(("Provider Turn ID", task.session.providerTurnId ?? "—", false)) }
        if fieldVisible("lastEventAt") { items.append(("最后事件", task.lastEventAt.formatted(), false)) }
        return items
    }
    private var note: String {
        switch task.status {
        case .done: "确认后归档本轮"
        case .idle: "最近没有新的活动"
        default: provider == .codex
            ? "状态粒度由当前 Hook / Connector 能力决定"
            : "只显示 Runtime 已验证的工具与计划事件"
        }
    }
    private var providerName: String {
        switch provider { case .claude: "Claude"; case .codex: "Codex"; case .gemini: "Gemini" }
    }
    private var badge: String {
        switch task.status {
        case .waiting: "等待"
        case .running: "运行中"
        case .failed: "出错"
        case .done: "完成"
        case .idle: "空闲"
        }
    }
    private var rightStatus: String {
        switch task.status {
        case .waiting:
            let since = task.oldestOpenOutboxAt ?? task.activitySince ?? task.lastEventAt
            let verb: String
            switch task.primaryAttentionKind {
            case .approval: verb = "等待批准"
            case .nativeApproval: verb = "原界面请求"
            case .question: verb = "等待回答"
            case .completion: verb = "等待确认"
            case .error: verb = "需要处理"
            case nil: verb = task.session.execState == "awaiting_approval" ? "等待批准" : "等待处理"
            }
            return "\(verb) · 已等 \(ZhFormat.waitDuration(model.now.timeIntervalSince(since)))"
        case .running:
            return "\(task.activity ?? "正在运行") · \(turnTiming)"
        case .failed:
            return "运行失败 · \(ZhFormat.relativeAgo(model.now.timeIntervalSince(task.lastEventAt)))"
        case .done:
            return "本轮已完成 · \(ZhFormat.relativeAgo(model.now.timeIntervalSince(task.lastEventAt)))"
        case .idle:
            return "最近活动 · \(ZhFormat.relativeAgo(model.now.timeIntervalSince(task.lastEventAt)))"
        }
    }
    private var turnTiming: String {
        let started = task.turnStartedAt ?? task.activitySince ?? task.lastEventAt
        let ended = task.turnEndedAt ?? model.now
        let total = ZhFormat.waitDuration(max(0, ended.timeIntervalSince(started)))
        if task.turnEndedAt == nil,
           let phase = task.activitySince,
           phase > started {
            return "本轮 \(total) · 当前阶段 \(ZhFormat.waitDuration(max(0, model.now.timeIntervalSince(phase))))"
        }
        return "本轮 \(total)"
    }
    private var rightColor: Color {
        switch task.status {
        case .waiting: DT.amberText
        case .running: DT.blueText
        case .failed: DT.redText
        case .done: DT.textSecondary
        case .idle: DT.textWeak
        }
    }
    private var titleColor: Color {
        switch task.status {
        case .done: DT.textSecondary
        case .idle: DT.textWeak
        default: DT.textPrimary
        }
    }
    private var rowBackground: Color {
        switch task.status {
        case .waiting: DT.cardStrong.opacity(0.92)
        case .running, .done: DT.cardMedium
        case .failed: DT.redBg.opacity(0.4)
        case .idle: DT.cardFaint
        }
    }
    private var borderColor: Color {
        if model.pinnedSessionId == task.id { return DT.blueBadgeStroke }
        switch task.status {
        case .waiting: return DT.amberStroke
        case .running: return DT.blueBadgeStroke.opacity(0.8)
        case .failed: return DT.redStroke
        case .done, .idle: return DT.hairlineSoft
        }
    }
}

private struct DetailLine: View {
    let label: String
    let value: String
    var emphasized = false

    var body: some View {
        HStack(spacing: 10) {
            Text(label).foregroundStyle(DT.textWeak)
            Spacer(minLength: 4)
            Text(value)
                .fontWeight(emphasized ? .semibold : .regular)
                .foregroundStyle(emphasized ? DT.amberText : DT.textPrimary)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity)
    }
}

private struct ProgressTrack: View {
    let fraction: Double
    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(DT.progressTrack)
                Capsule().fill(DT.blue)
                    .frame(width: proxy.size.width * max(0, min(1, fraction)))
            }
        }
    }
}

private struct ClearTaskButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 9.5, weight: .semibold))
            .foregroundStyle(DT.textSecondary)
            .padding(.horizontal, 10)
            .padding(.vertical, 2.5)
            .background(DT.cardMedium, in: Capsule())
            .overlay(Capsule().strokeBorder(DT.neutralBadgeStroke, lineWidth: 1))
            .opacity(configuration.isPressed ? 0.72 : 1)
    }
}

// MARK: - Quota

struct QuotaSection: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("QUOTA")
                    .font(.system(size: 13, weight: .heavy))
                    .kerning(0.65)
                    .foregroundStyle(DT.textPrimary)
                Text("额度余量")
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textSecondary)
            }

            if model.setupInfo == nil {
                SetupDetectionState()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if model.isFirstRun {
                FirstRunQuotaState()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if model.derived.quotaSlots.isEmpty {
                VStack(spacing: 7) {
                    Text("—")
                        .font(.system(size: 24, weight: .light))
                        .foregroundStyle(DT.textFaint)
                    Text("暂时没有额度数据")
                        .font(.system(size: 11.5, weight: .semibold))
                        .foregroundStyle(DT.textSecondary)
                    Text("完成一次 Agent 对话后会同步可验证额度")
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if snapshotRendering {
                quotaList
            } else {
                ScrollView(.vertical, showsIndicators: true) { quotaList }
                    .scrollBounceBehavior(.basedOnSize)
            }

        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .liquidGlassSurface(
            tint: DT.cardSoft.opacity(0.18),
            radius: 24,
            stroke: DT.hairline,
            shadow: DT.cardShadow.opacity(0.8),
            shadowRadius: 30,
            shadowY: 12
        )
    }

    private var quotaList: some View {
        LazyVStack(spacing: 8) {
            ForEach(model.derived.quotaSlots) { slot in
                QuotaCard(slot: slot)
            }
        }
        .padding(.top, 11)
    }
}

private struct FirstRunQuotaState: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 8) {
            ForEach(model.setupInfo?.providers ?? []) { provider in
                HStack(spacing: 8) {
                    ProviderAvatar(
                        kind: ProviderKind(record: provider.provider) ?? .codex,
                        size: 18
                    )
                    Text(provider.provider == "claude" ? "Claude" : "Codex")
                        .font(.system(size: 10.5, weight: .bold))
                        .foregroundStyle(DT.textSecondary)
                    Spacer()
                    Chip(text: "未接入", tone: .neutral, fontSize: 8.5)
                }
                .padding(10)
                .background(DT.cardFaint, in: RoundedRectangle(cornerRadius: 11))
            }
            Text("安全接入并产生真实会话后读取可验证额度")
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textWeak)
                .multilineTextAlignment(.center)
        }
        .padding(.top, 12)
    }
}

private struct QuotaCard: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.openSettings) private var openSettings
    let slot: QuotaSlot

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 7) {
                ProviderAvatar(kind: slot.slot.provider, size: 16)
                Text(title)
                    .font(.system(size: 11.5, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
                Spacer(minLength: 3)
                statusChip
            }
            HStack(spacing: 6) {
                Text(sourceLabel)
                if let plan = slot.planType, !plan.isEmpty { Text(plan) }
            }
            .font(.system(size: 8.5, weight: .semibold))
            .foregroundStyle(DT.textFaint)
            .padding(.top, 4)
            content
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(cardFill, in: RoundedRectangle(cornerRadius: 13))
        .overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(DT.hairlineSoft, lineWidth: 1))
        .shadow(color: DT.softShadow, radius: 2, y: 1)
    }

    @ViewBuilder
    private var content: some View {
        switch slot.availability {
        case .available(let remaining, let resetsAt, let capturedAt):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("剩余 \(Int(remaining.rounded()))%")
                    .font(.system(size: 16, weight: .heavy))
                    .foregroundStyle(remaining < 50 ? DT.amberText : DT.textPrimary)
                Spacer(minLength: 2)
                Text(resetsAt.map(resetText) ?? "重置时间未提供")
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
            }
            .padding(.top, 8)
            ProgressTrack(fraction: remaining / 100)
                .frame(height: 5)
                .tint(remaining < 50 ? Color.orange : DT.greenDot)
                .padding(.top, 6)
                .overlay {
                    GeometryReader { proxy in
                        HStack(spacing: 0) {
                            Capsule().fill(remaining < 50 ? Color.orange : DT.greenDot)
                                .frame(width: proxy.size.width * max(0, min(1, remaining / 100)))
                            Spacer(minLength: 0)
                        }
                    }
                    .padding(.top, 6)
                }
            Text(capturedAt.map { "\(max(0, Int(model.now.timeIntervalSince($0) / 60))) 分钟前更新" } ?? "更新时间未提供")
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textFaint)
                .padding(.top, 6)
        case .stale(let remaining, let resetsAt, let capturedAt):
            Text(remaining.map { "上次记录剩余 \(Int($0.rounded()))%" } ?? "额度数据已过期")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(DT.amberText)
                .padding(.top, 8)
            Text([resetsAt.map(resetText), capturedAt.map { ZhFormat.relativeAgo(model.now.timeIntervalSince($0)) }]
                .compactMap { $0 }.joined(separator: " · "))
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textFaint)
                .padding(.top, 6)
        case .unavailable(let reason):
            Text(reason ?? "当前 Provider 版本暂不支持额度解析")
                .font(.system(size: 10.5))
                .foregroundStyle(DT.textSecondary)
                .lineSpacing(2)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 8)
            Button("检查设置") { openSettings() }
                .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                .padding(.top, 8)
        }
    }

    @ViewBuilder
    private var statusChip: some View {
        switch slot.availability {
        case .available:
            Chip(text: "可用", tone: .green, fontSize: 8.5)
        case .stale:
            Chip(text: "已过期", tone: .amber, fontSize: 8.5)
        case .unavailable:
            Chip(text: "暂不可用", tone: .neutral, fontSize: 8.5)
        }
    }

    private var title: String {
        let provider = slot.slot.provider == .claude ? "Claude" : "Codex"
        return "\(provider) · \(slot.title)"
    }
    private var sourceLabel: String {
        switch slot.source {
        case "oauth_usage": "OAuth 自动同步"
        case "statusline": "Claude 对话同步"
        case "rollout_experimental": "本机 Session 同步"
        default: slot.source.replacingOccurrences(of: "_", with: " ")
        }
    }
    private var cardFill: Color {
        if case .unavailable = slot.availability { return DT.cardFaint }
        return DT.cardMedium
    }
    private func resetText(_ date: Date) -> String {
        let base = ZhFormat.resetTime(date, now: model.now)
        return Calendar.current.isDateInToday(date) ? "今天 \(base)" : base
    }
}
