import ActRealmKit
import SwiftUI

struct OutboxSection: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering

    private var entries: [OutboxEntry] { model.derived.openOutbox }
    private var selectedIndex: Int {
        guard !entries.isEmpty else { return 0 }
        return min(model.outboxPageIndex, entries.count - 1)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("OUTBOX")
                    .font(.system(size: 13, weight: .heavy))
                    .kerning(0.65)
                    .foregroundStyle(DT.textPrimary)
                Text("需要处理")
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textSecondary)
                Spacer()
                Text("\(entries.count)")
                    .font(.system(size: 11, weight: .bold))
                    .foregroundStyle(DT.amberText)
                    .frame(minWidth: 20)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 1.5)
                    .background(DT.amberBg, in: Capsule())
                    .overlay(Capsule().strokeBorder(DT.amberStroke, lineWidth: 1))
            }
            Text(subtitle)
                .font(.system(size: 10.5))
                .foregroundStyle(DT.textWeak)
                .padding(.top, 3)

            if entries.isEmpty {
                OutboxEmpty()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if snapshotRendering {
                outboxContent
            } else {
                ScrollView(.vertical, showsIndicators: true) { outboxContent }
                    .contentMargins(.horizontal, 0)
                    .scrollBounceBehavior(.basedOnSize)
            }

            if let pending = model.derived.pendingDecision {
                UndoBar(pending: pending)
                    .padding(.top, 8)
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .mainLaneSurface(
            radius: 24,
            stroke: DT.hairline,
            shadow: DT.cardShadow.opacity(0.8),
            shadowRadius: 30,
            shadowY: 12
        )
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .strokeBorder(
                    model.outboxHighlighted ? DT.amberStroke.opacity(0.9) : .clear,
                    lineWidth: model.outboxHighlighted ? 2 : 0
                )
        )
        .shadow(
            color: model.outboxHighlighted ? DT.amberDot.opacity(0.26) : .clear,
            radius: 18
        )
        .animation(.easeInOut(duration: 0.25), value: model.outboxHighlighted)
    }

    private var outboxContent: some View {
        LazyVStack(alignment: .leading, spacing: 0) {
            OutboxPrimaryCard(entry: entries[selectedIndex])
                .id(entries[selectedIndex].id)
                .padding(.top, 14)

            let rest = entries.enumerated().filter { $0.offset != selectedIndex }
            if !rest.isEmpty {
                Text("队列 · 还有 \(rest.count) 项")
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textWeak)
                    .padding(.top, 16)
                    .padding(.bottom, 8)
                ForEach(rest, id: \.element.id) { index, entry in
                    QueueRow(entry: entry) {
                        withAnimation(.easeOut(duration: 0.2)) {
                            model.outboxPageIndex = index
                            model.revealSession(for: entry)
                        }
                    }
                    .padding(.bottom, 7)
                }
            }
        }
        .padding(.bottom, 6)
    }

    private var subtitle: String {
        guard let wait = model.derived.longestWait else { return "暂无需要处理的事项" }
        return "最久等待 \(max(0, Int(wait / 60))) 分钟"
    }
}

private struct OutboxPrimaryCard: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    let entry: OutboxEntry
    @State private var confirming = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 7) {
                Chip(text: entry.kind.badgeText, tone: .forOutboxKind(entry.kind), fontSize: 10)
                Spacer()
                Text(waitText)
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textWeak)
            }

            Text(entry.actionTitle)
                .font(.system(size: 16, weight: .heavy))
                .foregroundStyle(DT.textStrong)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 11)

            HStack(spacing: 7) {
                if let provider = entry.provider {
                    ProviderAvatar(kind: provider, size: 18)
                }
                Text(providerName)
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textSecondary)
                    .fixedSize()
                Text(entry.taskTitle.map { "任务：\($0)" } ?? "")
                    .font(.system(size: 11))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
            }
            .padding(.top, 7)

            if let returnNote = model.foregroundReturnNotes[entry.id] {
                HStack(spacing: 7) {
                    Image(systemName: "arrow.uturn.backward")
                        .font(.system(size: 9, weight: .bold))
                    Text(returnNote)
                        .font(.system(size: 10.5, weight: .semibold))
                }
                .foregroundStyle(DT.amberText)
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(DT.amberBg.opacity(0.7), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(DT.amberStroke, lineWidth: 1))
                .padding(.top, 9)
            }

            switch entry.kind {
            case .approval:
                approvalBody
            case .nativeApproval:
                nativeApprovalBody
            case .question:
                questionBody
            case .completion:
                completionBody
            case .error:
                errorBody
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 16)
        .padding(.bottom, 15)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(cardBackground, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(cardBorder, lineWidth: 1)
        )
        .shadow(color: DT.cardShadow.opacity(0.55), radius: 20, y: 8)
        .animation(.easeOut(duration: 0.2), value: confirming)
    }

    private var approvalBody: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Text(entry.toolName ?? "操作")
                    .font(.system(size: 10.5, weight: .bold))
                    .foregroundStyle(DT.textSecondary)
                    .padding(.horizontal, 9)
                    .padding(.vertical, 2.5)
                    .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 7))
                Text(entry.attention.commandPreview ?? "Provider 未提供命令预览")
                    .font(.system(size: 13, design: .monospaced))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .padding(.horizontal, 13)
            .padding(.vertical, 11)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(DT.cardStrong.opacity(0.82), in: RoundedRectangle(cornerRadius: 13))
            .overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(DT.hairline, lineWidth: 1))
            .padding(.top, 11)

            Text(expiryLine)
                .font(.system(size: 11))
                .foregroundStyle(DT.textSecondary)
                .padding(.top, 9)

            if entry.state != .open {
                Text(entry.state == .committing ? "决定将在 3 秒撤回窗口后提交" : "决定已写给 Provider，等待后续事件确认")
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.amberText)
                    .padding(.top, 12)
            } else if confirming {
                HStack(spacing: 8) {
                    Text("确认允许运行这条命令？")
                        .font(.system(size: 11, weight: .bold))
                        .foregroundStyle(DT.redText)
                    Spacer(minLength: 2)
                    Button("确认允许") { model.approve(entry); confirming = false }
                        .buttonStyle(ActionButtonStyle(kind: .danger, compact: true))
                    Button("取消") { confirming = false }
                        .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 8)
                .background(DT.redBg.opacity(0.72), in: RoundedRectangle(cornerRadius: 12))
                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(DT.redStroke, lineWidth: 1))
                .padding(.top, 12)
            } else {
                HStack(spacing: 9) {
                    Button("允许") { model.approve(entry) }
                        .buttonStyle(ActionButtonStyle(kind: .primary))
                    Button("拒绝") { model.deny(entry) }
                        .buttonStyle(ActionButtonStyle(kind: .secondary))
                    Button("二次确认后允许") { confirming = true }
                        .buttonStyle(ActionButtonStyle(kind: .tertiary))
                }
                .padding(.top, 14)
            }
        }
    }

    private var questionBody: some View {
        Group {
            if let prompt = entry.attention.interaction,
               entry.state == .open,
               entry.attention.requestId != nil {
                InteractiveQuestionView(entry: entry, prompt: prompt)
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    Text(entry.attention.detail ?? "Agent 正在等待回答。")
                        .font(.system(size: 11.5))
                        .foregroundStyle(DT.textSecondary)
                        .lineSpacing(3)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 11)
                        .padding(.vertical, 9)
                        .background(DT.blueBg.opacity(0.7), in: RoundedRectangle(cornerRadius: 10))
                        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(DT.blueBadgeStroke, lineWidth: 1))
                    Text("当前没有可用的直接回复通道；ActRealm 不会把复制文字伪装成已回答。")
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                    Button("返回 Agent 原窗口") {
                        Task { await model.jump(to: entry) }
                    }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                }
                .padding(.top, 9)
            }
        }
    }

    private var nativeApprovalBody: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(entry.attention.detail ?? "此请求由 Provider 原界面拥有；ActRealm 只同步等待与解决状态。")
                .font(.system(size: 11.5))
                .foregroundStyle(DT.textSecondary)
                .lineSpacing(3)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(DT.amberBg.opacity(0.62), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(DT.amberStroke, lineWidth: 1))
            Button("打开应用") {
                Task { await model.jump(to: entry) }
            }
            .buttonStyle(ActionButtonStyle(kind: .primary, compact: true))
        }
        .padding(.top, 10)
    }

    private var completionBody: some View {
        VStack(spacing: 12) {
            Text(entry.attention.detail ?? "本轮修改已完成，等待确认。")
                .font(.system(size: 11.5))
                .foregroundStyle(DT.textSecondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 11)
                .padding(.vertical, 9)
                .background(DT.greenBg.opacity(0.7), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(DT.greenStroke, lineWidth: 1))
            HStack(spacing: 7) {
                Button("确认完成") { model.acknowledge(entry) }
                    .buttonStyle(ActionButtonStyle(kind: .success))
                Button("打开应用") {
                    Task { await model.jump(to: entry) }
                }
                .buttonStyle(ActionButtonStyle(kind: .secondary))
            }
        }
        .padding(.top, 9)
    }

    private var errorBody: some View {
        VStack(spacing: 12) {
            Text(entry.attention.detail ?? "Provider 未提供更多错误信息。")
                .font(.system(size: 11.5))
                .foregroundStyle(DT.redText)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 11)
                .padding(.vertical, 9)
                .background(DT.redBg.opacity(0.65), in: RoundedRectangle(cornerRadius: 10))
            HStack(spacing: 7) {
                Button("标记已解决") { model.acknowledge(entry) }
                    .buttonStyle(ActionButtonStyle(kind: .primary))
                Button("稍后提醒") { model.snooze(entry) }
                    .buttonStyle(ActionButtonStyle(kind: .secondary))
            }
        }
        .padding(.top, 9)
    }

    private var waitText: String {
        "已等 \(ZhFormat.waitDuration(model.now.timeIntervalSince(entry.createdAt)))"
    }

    private var expiryLine: String {
        guard let expiresAt = entry.expiresAt else { return "等待处理" }
        let minutes = max(0, Int(round(expiresAt.timeIntervalSince(model.now) / 60)))
        return "等待处理 · \(minutes) 分钟后过期"
    }

    private var providerName: String {
        entry.provider?.displayName ?? entry.attention.provider
    }

    private var cardBackground: AnyShapeStyle {
        switch entry.kind {
        case .approval, .nativeApproval:
            AnyShapeStyle(LinearGradient(
                colors: [DT.amberBg.opacity(0.72), DT.cardStrong.opacity(0.76)],
                startPoint: .top, endPoint: .bottom
            ))
        case .question:
            AnyShapeStyle(LinearGradient(
                colors: [DT.blueBg.opacity(0.55), DT.cardStrong.opacity(0.78)],
                startPoint: .top, endPoint: .bottom
            ))
        case .completion:
            AnyShapeStyle(LinearGradient(
                colors: [DT.greenBg.opacity(0.55), DT.cardStrong.opacity(0.78)],
                startPoint: .top, endPoint: .bottom
            ))
        case .error:
            AnyShapeStyle(LinearGradient(
                colors: [DT.redBg.opacity(0.6), DT.cardStrong.opacity(0.78)],
                startPoint: .top, endPoint: .bottom
            ))
        }
    }

    private var cardBorder: Color {
        switch entry.kind {
        case .approval, .nativeApproval: DT.amberStroke
        case .question: DT.blueBadgeStroke
        case .completion: DT.greenStroke
        case .error: DT.redStroke
        }
    }
}

private struct QueueRow: View {
    @EnvironmentObject private var model: AppModel
    let entry: OutboxEntry
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                Chip(text: entry.kind.badgeText, tone: .forOutboxKind(entry.kind), fontSize: 9.5)
                Text(entry.actionTitle)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
                Spacer(minLength: 2)
                Text(ZhFormat.shortAge(model.now.timeIntervalSince(entry.createdAt)))
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textWeak)
            }
            .padding(.horizontal, 13)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(DT.cardMedium, in: RoundedRectangle(cornerRadius: 13))
            .overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(DT.hairline, lineWidth: 1))
            .shadow(color: DT.softShadow, radius: 2, y: 1)
        }
        .buttonStyle(.plain)
    }
}

private struct OutboxEmpty: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        VStack(spacing: 10) {
            Text(model.setupInfo == nil ? "…" : "✓")
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(model.setupInfo == nil ? DT.textWeak : DT.greenText)
                .frame(width: 52, height: 52)
                .background(model.setupInfo == nil ? DT.cardFaint : DT.greenBg, in: Circle())
                .overlay(Circle().strokeBorder(model.setupInfo == nil ? DT.hairline : DT.greenStroke, lineWidth: 1))
            Text(model.setupInfo == nil
                ? "正在检测本机 Agent"
                : model.isFirstRun ? "还没有需要处理的事项" : "全部处理完毕")
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(DT.textSecondary)
            Text(model.setupInfo == nil
                ? "接入状态确认前不会显示缓存或演示数据"
                : model.isFirstRun
                    ? "连接 Agent 后，审批、提问和完成确认会出现在这里"
                    : "新事件会先以 HUD 胶囊出现")
                .font(.system(size: 10.5))
                .foregroundStyle(DT.textWeak)
        }
        .multilineTextAlignment(.center)
    }
}

private struct UndoBar: View {
    @EnvironmentObject private var model: AppModel
    let pending: PendingDecision

    var body: some View {
        HStack(spacing: 9) {
            ZStack {
                ConicRing(fraction: fraction, color: DT.greenText, size: 30, lineWidth: 3, coreBackground: DT.greenBg)
                Text(secondsLabel)
                    .font(.system(size: 8, weight: .heavy))
                    .foregroundStyle(DT.greenText)
            }
            Text(pending.summary)
                .font(.system(size: 10.5, weight: .bold))
                .foregroundStyle(DT.textPrimary)
                .lineLimit(1)
            Text(phaseText)
                .font(.system(size: 10.5))
                .foregroundStyle(DT.textSecondary)
                .lineLimit(1)
            Spacer(minLength: 0)
            if case .undoable = pending.phase {
                Button("撤回") { model.undoPendingDecision() }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(DT.greenBg.opacity(0.8), in: Capsule())
        .overlay(Capsule().strokeBorder(DT.greenStroke, lineWidth: 1))
    }

    private var remaining: TimeInterval {
        guard case .undoable(let deadline) = pending.phase else { return 0 }
        return max(0, deadline.timeIntervalSince(model.now))
    }
    private var fraction: Double { min(1, remaining / DerivedState.undoWindow) }
    private var secondsLabel: String { "\(Int(ceil(remaining)))s" }
    private var phaseText: String {
        switch pending.phase {
        case .undoable: "· 尚未写给 Provider"
        case .sent: "· 已写给 Provider，等待后续事件"
        case .confirmed: "· Provider 后续事件已确认继续"
        }
    }
}

struct ActionButtonStyle: ButtonStyle {
    enum Kind { case primary, secondary, tertiary, danger, success }
    let kind: Kind
    var compact = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: compact ? 11 : 12, weight: .bold))
            .foregroundStyle(foreground)
            .padding(.horizontal, compact ? 13 : 8)
            .padding(.vertical, compact ? 6 : 10)
            .frame(maxWidth: compact ? nil : .infinity)
            .background(background, in: Capsule())
            .overlay(Capsule().strokeBorder(stroke, lineWidth: stroke == .clear ? 0 : 1))
            .shadow(color: shadow, radius: compact ? 8 : 10, y: compact ? 3 : 5)
            .opacity(configuration.isPressed ? 0.82 : 1)
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
    }

    private var foreground: Color {
        switch kind {
        case .primary, .danger, .success: .white
        case .secondary: DT.textPrimary
        case .tertiary: DT.textSecondary
        }
    }
    private var background: AnyShapeStyle {
        switch kind {
        case .primary: AnyShapeStyle(DT.primaryGradient)
        case .danger: AnyShapeStyle(LinearGradient(colors: [Color.red.opacity(0.72), Color.red], startPoint: .top, endPoint: .bottom))
        case .success: AnyShapeStyle(LinearGradient(colors: [Color.green.opacity(0.72), DT.greenDot], startPoint: .top, endPoint: .bottom))
        case .secondary: AnyShapeStyle(DT.cardStrong)
        case .tertiary: AnyShapeStyle(DT.cardMedium)
        }
    }
    private var stroke: Color {
        switch kind {
        case .secondary, .tertiary: DT.neutralBadgeStroke
        default: .clear
        }
    }
    private var shadow: Color {
        switch kind {
        case .primary: DT.blue.opacity(0.32)
        case .danger: DT.redText.opacity(0.28)
        case .success: DT.greenDot.opacity(0.28)
        case .secondary, .tertiary: DT.softShadow
        }
    }
}
