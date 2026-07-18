import ActRealmKit
import SwiftUI

struct AgentTasksSection: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering

    private var tasks: [LaneTask] {
        model.derived.agentTasks.filter { !model.isTaskDismissed($0) }
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
                Text("没有正在进行的任务")
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textWeak)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
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
        return "\(tasks.count) 个任务 · \(waiting) 等你 · \(running) 在跑 · \(completed) 刚完成"
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
                Text(task.title)
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(titleColor)
                    .lineLimit(1)
                Chip(text: badge, tone: .forStatus(task.status), fontSize: 9.5)
                Spacer(minLength: 5)
                Text(rightStatus)
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(rightColor)
                    .lineLimit(1)
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

            HStack(spacing: 7) {
                Text("\(providerName) · \(task.model ?? "未知模型")")
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
                if let plan = task.planProgress {
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
                        ? "请先处理 \(task.openOutboxCount) 项待处理事项"
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
            Grid(horizontalSpacing: 24, verticalSpacing: 6) {
                GridRow {
                    DetailLine(label: "工作区", value: workspace)
                    DetailLine(label: "上下文用量", value: contextText, emphasized: contextIsTight)
                }
                GridRow {
                    DetailLine(label: "计划", value: planText)
                    DetailLine(label: "Token 用量", value: tokenText, emphasized: tokenIsHigh)
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

    private var promptPreview: String? { task.session.title }
    private var workspace: String {
        switch provider {
        case .claude: "Claude Code CLI"
        case .codex: "Codex CLI"
        case .gemini: "Gemini CLI"
        }
    }
    private var contextText: String {
        if let fraction = task.contextUsageFraction { return "\(Int((fraction * 100).rounded()))%" }
        if let tokens = task.inputTokens { return ZhFormat.tokenCount(tokens) }
        return "暂无数据"
    }
    private var contextIsTight: Bool { (task.contextUsageFraction ?? 0) >= 0.7 }
    private var planText: String {
        if let plan = task.planProgress { return "\(plan.done)/\(plan.total)（进行中）" }
        return provider == .codex ? "未提供计划事件" : "未提供"
    }
    private var tokenText: String { task.totalTokens.map(ZhFormat.tokenCount) ?? "暂无数据" }
    private var tokenIsHigh: Bool { (task.totalTokens ?? 0) >= 140_000 }
    private var note: String {
        switch task.status {
        case .done: "确认后归档本轮"
        case .idle: "最近没有新的活动"
        default: provider == .codex ? "Codex 仅提供轮级状态，无工具级细节" : "Claude 提供工具级事件与计划"
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
            let verb = task.session.execState == "awaiting_approval" ? "等待批准" : "等待回答"
            return "\(verb) · 已等 \(ZhFormat.waitDuration(model.now.timeIntervalSince(since)))"
        case .running:
            let elapsed = max(0, Int(model.now.timeIntervalSince(task.activitySince ?? task.lastEventAt)))
            return "\(task.activity ?? "正在运行") · \(elapsed) 秒"
        case .failed:
            return "运行失败 · \(ZhFormat.relativeAgo(model.now.timeIntervalSince(task.lastEventAt)))"
        case .done:
            return "本轮已完成 · \(ZhFormat.relativeAgo(model.now.timeIntervalSince(task.lastEventAt)))"
        case .idle:
            return "最近活动 · \(ZhFormat.relativeAgo(model.now.timeIntervalSince(task.lastEventAt)))"
        }
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
    let onOpenRuntime: () -> Void

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

            if snapshotRendering {
                quotaList
            } else {
                ScrollView(.vertical, showsIndicators: true) { quotaList }
                    .scrollBounceBehavior(.basedOnSize)
            }

            RuntimeFooter(action: onOpenRuntime)
                .padding(.top, 10)
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
        let window: String
        switch slot.slot {
        case .claude5h: window = "5 小时"
        case .claude7d: window = "7 天"
        case .codexWeek: window = "本周"
        }
        return "\(provider) · \(window)"
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

private struct RuntimeFooter: View {
    @EnvironmentObject private var model: AppModel
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 6) {
                    StatusDot(
                        color: model.bridgeStatus.isListening ? DT.greenDot : DT.redText,
                        size: 6,
                        glow: model.bridgeStatus.isListening
                    )
                    Text(model.bridgeStatus.isListening ? "Runtime · 本机在线" : "Runtime · 未连接")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(DT.textSecondary)
                }
                Text("最近同步 · \(model.lastSyncAt.map(ZhFormat.syncClock) ?? "--:--:--")")
                Text("数据仅在这台 Mac").underline()
            }
            .font(.system(size: 10))
            .foregroundStyle(DT.textWeak)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.top, 10)
            .overlay(alignment: .top) {
                Rectangle().fill(DT.separator).frame(height: 1)
            }
        }
        .buttonStyle(.plain)
        .help("打开 Runtime 监控、诊断与重启")
    }
}
