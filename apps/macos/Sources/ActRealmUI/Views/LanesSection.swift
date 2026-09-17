import ActRealmKit
import SwiftUI

private struct AgentTaskPresentation: Identifiable {
    let task: LaneTask

    var id: String { task.id }
}

enum AgentTaskPresentationOrdering {
    static func ordered<Value>(
        _ values: [Value],
        status: (Value) -> LaneTaskStatus,
        priority: (Value) -> Int,
        oldestOpenOutboxAt: (Value) -> Date?,
        turnStartedAt: (Value) -> Date?,
        lastEventAt: (Value) -> Date,
        isPinned: (Value) -> Bool,
        id: (Value) -> String
    ) -> [Value] {
        values.sorted { lhs, rhs in
            let leftStatus = status(lhs)
            let leftRank = priority(lhs)
            let rightRank = priority(rhs)
            if leftRank != rightRank {
                return leftRank < rightRank
            }

            switch leftStatus {
            case .waiting:
                let leftWaitingSince = oldestOpenOutboxAt(lhs) ?? lastEventAt(lhs)
                let rightWaitingSince = oldestOpenOutboxAt(rhs) ?? lastEventAt(rhs)
                if leftWaitingSince != rightWaitingSince {
                    return leftWaitingSince < rightWaitingSince
                }
            case .running:
                if isPinned(lhs) != isPinned(rhs) {
                    return isPinned(lhs)
                }
                let leftTurnStartedAt = turnStartedAt(lhs) ?? .distantPast
                let rightTurnStartedAt = turnStartedAt(rhs) ?? .distantPast
                if leftTurnStartedAt != rightTurnStartedAt {
                    return leftTurnStartedAt > rightTurnStartedAt
                }
            case .failed, .done, .idle:
                if isPinned(lhs) != isPinned(rhs) {
                    return isPinned(lhs)
                }
                if lastEventAt(lhs) != lastEventAt(rhs) {
                    return lastEventAt(lhs) > lastEventAt(rhs)
                }
            }

            return id(lhs) < id(rhs)
        }
    }

}


struct AgentTasksSection: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    @Environment(\.locale) private var locale
    let onOpenSetup: () -> Void

    private var tasks: [AgentTaskPresentation] {
        let local = model.visibleAgentTasks.map {
            AgentTaskPresentation(task: $0)
        }
        return AgentTaskPresentationOrdering.ordered(
            local,
            status: \.task.status,
            priority: \.task.taskPriorityRank,
            oldestOpenOutboxAt: \.task.oldestOpenOutboxAt,
            turnStartedAt: \.task.turnStartedAt,
            lastEventAt: \.task.lastEventAt,
            isPinned: {
                model.expandedTaskId == $0.id || model.pinnedSessionId == $0.id
            },
            id: \.id
        )
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    headerTitle
                    Text("当前任务 · 点击展开详情")
                        .font(.system(size: 11))
                        .foregroundStyle(DT.textSecondary)
                        .fixedSize(horizontal: true, vertical: false)
                    Spacer(minLength: 8)
                    Text(summary)
                        .font(.system(size: 10.5))
                        .foregroundStyle(DT.textWeak)
                        .fixedSize(horizontal: true, vertical: false)
                }

                VStack(alignment: .leading, spacing: 4) {
                    headerTitle
                    Text(summary)
                        .font(.system(size: 10.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            if tasks.isEmpty {
                if model.setupInfo == nil {
                    SetupDetectionState()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if model.isFirstRun {
                    FirstRunTasksEmpty(onOpenSetup: onOpenSetup)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Text("当前没有可见任务")
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
        .mainLaneSurface(
            radius: 24,
            stroke: DT.hairline,
            shadow: DT.cardShadow.opacity(0.8),
            shadowRadius: 30,
            shadowY: 12
        )
    }

    private var taskList: some View {
        // Runtime projects only the small active-task set. An eager stack is
        // intentional here: a tall expanded card inside LazyVStack can enter
        // a macOS SwiftUI estimate/scroll feedback loop and continuously
        // rebuild the row's native controls.
        VStack(spacing: 6) {
            ForEach(tasks) { presentation in
                TaskRow(
                    task: presentation.task,
                    expanded: model.expandedTaskId == presentation.id
                )
                .id(presentation.id)
            }
        }
        .padding(.top, 11)
    }

    private var summary: String {
        let waiting = tasks.filter { $0.task.status == .waiting && !$0.task.awaitingCompletionConfirmation }.count
        let confirmation = tasks.filter { $0.task.awaitingCompletionConfirmation }.count
        let running = tasks.filter { $0.task.status == .running }.count
        let completed = tasks.filter { $0.task.status == .done }.count
        let base = localizedFormat(
            "%lld 个任务 · %lld 等待 · %lld 运行中 · %lld 已完成",
            locale: locale,
            Int64(tasks.count),
            Int64(waiting),
            Int64(running),
            Int64(completed)
        )
        return confirmation > 0
            ? base + " · " + localizedFormat("%lld 完成待确认", locale: locale, Int64(confirmation))
            : base
    }

    private var compactSummary: String {
        localizedFormat(
            "%lld 个任务 · %lld 等待",
            locale: locale,
            Int64(tasks.count),
            Int64(tasks.filter { $0.task.status == .waiting }.count)
        )
    }

    private var headerTitle: some View {
        Text("AGENT TASKS")
            .font(.system(size: 13, weight: .heavy))
            .kerning(0.65)
            .foregroundStyle(DT.textPrimary)
            .fixedSize(horizontal: true, vertical: false)
    }
}

private struct SetupDetectionState: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(spacing: 9) {
            if model.bridgeStatus.isListening {
                ProgressView().controlSize(.small)
            } else {
                Image(systemName: "bolt.horizontal.circle")
                    .font(.system(size: 25, weight: .light))
                    .foregroundStyle(DT.textWeak)
            }
            Text(localized(
                model.bridgeStatus.isListening ? "正在检测本机 Agent" : "正在等待 Runtime",
                locale: locale
            ))
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
    @Environment(\.locale) private var locale
    let task: LaneTask
    let expanded: Bool
    @State private var showingTaskActions = false
    @State private var workflowEvents: [RuntimeTimelineEvent] = []
    @State private var workflowLoading = false
    @State private var workflowLoadingEarlier = false
    @State private var workflowError: String?
    @State private var workflowEarlierError: String?
    @State private var workflowHasMore = false
    @State private var workflowReachedDisplayLimit = false
    @State private var showingAllRecentActivity = false
    @State private var reviewSnapshot: RuntimeTaskReviewSnapshot?
    @State private var reviewLoading = false
    @State private var reviewError: String?
    @State private var showingReviewDiff = false
    @State private var showingCheckpoints = false
    @State private var moreDetailsExpanded = false

    private var provider: ProviderKind { ProviderKind(record: task.session.provider) ?? .codex }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: 7) {
                ProviderAvatar(kind: provider, size: 20)
                Text(fieldVisible("task")
                    ? task.localizedTitle(language: model.appLanguage)
                    : providerName)
                    .font(.system(size: 12.5, weight: .bold))
                    .foregroundStyle(titleColor)
                    .lineLimit(1)
                Chip(text: badge, tone: .forStatus(task.status), fontSize: 9.5)
                Spacer(minLength: 5)
                if fieldVisible("activity") {
                    TaskRelativeStatusLabel(
                        task: task,
                        language: model.appLanguage,
                        locale: locale
                    )
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(rightColor)
                        .multilineTextAlignment(.trailing)
                        .lineLimit(2)
                        .frame(maxWidth: 190, alignment: .trailing)
                        .fixedSize(horizontal: false, vertical: true)
                        .layoutPriority(1)
                }
            }

            if let prompt = promptPreview, !prompt.isEmpty {
                HStack(spacing: 6) {
                    Text(localized("任务摘要", locale: locale))
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

            if fieldVisible("activity"),
               task.localizedCurrentAction(language: model.appLanguage) != task.localizedState(language: model.appLanguage) {
                HStack(alignment: .firstTextBaseline, spacing: 7) {
                    Text(localized("当前动作", locale: locale))
                        .font(.system(size: 9.5, weight: .semibold))
                        .foregroundStyle(DT.textWeak)
                    Text(task.localizedCurrentAction(language: model.appLanguage))
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(DT.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                    if task.status == .running, task.session.execState == "tool_running",
                       let target = task.session.currentTarget, !target.isEmpty {
                        Text(target)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(DT.textSecondary)
                            .lineLimit(1).truncationMode(.middle)
                    }
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 5)
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
                    Text(TaskPlanPresentation.compactText(
                        done: plan.done,
                        total: plan.total,
                        steps: task.session.planSteps,
                        locale: locale
                    ))
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                    ProgressTrack(fraction: Double(plan.done) / Double(plan.total))
                        .frame(width: 70, height: 4)
                    if let activity = task.localizedActivity(language: model.appLanguage),
                       activity.contains("子 Agent") || activity.lowercased().contains("subagent") {
                        Text(activity)
                            .font(.system(size: 9.5))
                            .foregroundStyle(DT.textWeak)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 4)
                taskMenu
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
        .sheet(isPresented: $showingReviewDiff) {
            ReviewDiffSheet(
                client: model.client,
                sessionID: task.id,
                language: model.appLanguage
            )
        }
        .sheet(isPresented: $showingCheckpoints) {
            CheckpointSheet(
                client: model.client,
                sessionID: task.id,
                taskTitle: task.localizedTitle(language: model.appLanguage),
                language: model.appLanguage
            )
        }
        .task(id: workflowRefreshID) {
            await refreshWorkflowIfNeeded()
        }
        .task(id: reviewRefreshID) {
            if ProductScope.reviewEnabled {
                await refreshReviewIfNeeded()
            }
        }
    }

    private var taskMenu: some View {
        taskActionsButton(
            accessibilityLabel: localized("任务操作", locale: locale),
            help: localized(
                "移除当前卡片；该会话收到新事件后重新显示",
                locale: locale
            )
        ) {
            localTaskActions
        }
    }

    private var localTaskActions: some View {
        taskActionsPanel {
            Button(role: .destructive) {
                showingTaskActions = false
                model.deleteTaskCard(task)
            } label: {
                Label(localized("删除任务", locale: locale), systemImage: "trash")
            }
            .help(localized("仅删除任务卡，不停止 Agent，也不删除原会话和 Token 记录", locale: locale))
        }
    }

    private func taskActionsButton<Content: View>(
        accessibilityLabel: String,
        help: String,
        @ViewBuilder content: @escaping () -> Content
    ) -> some View {
        Button {
            showingTaskActions.toggle()
        } label: {
            Image(systemName: "ellipsis")
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(DT.textSecondary)
                .frame(width: 28, height: 20)
                .background(DT.cardMedium, in: Capsule())
                .overlay(Capsule().strokeBorder(DT.neutralBadgeStroke, lineWidth: 1))
                .accessibilityLabel(Text(accessibilityLabel))
        }
        .buttonStyle(.plain)
        .fixedSize()
        .help(help)
        .popover(
            isPresented: $showingTaskActions,
            attachmentAnchor: .rect(.bounds),
            arrowEdge: .bottom
        ) {
            content()
        }
    }

    private func taskActionsPanel<Content: View>(
        @ViewBuilder content: () -> Content
    ) -> some View {
        ScrollView(.vertical, showsIndicators: true) {
            VStack(alignment: .leading, spacing: 7) {
                content()
            }
            .buttonStyle(.borderless)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
        }
        .frame(width: 280)
        .frame(maxHeight: 440)
    }

    private func actionSectionLabel(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 9.5, weight: .semibold))
            .foregroundStyle(DT.textWeak)
            .textCase(.uppercase)
    }


    private var expandedDetails: some View {
        return VStack(alignment: .leading, spacing: 9) {
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

            if fieldVisible("taskFlow"), fieldVisible("workflow") {
                planAndWorkflowDetails
            } else {
                if fieldVisible("taskFlow") {
                    planDetails
                }
                if fieldVisible("workflow") {
                    workflowDetails
                }
            }

            if fieldVisible("subagents"), !task.session.subagents.isEmpty {
                subagentDetails
            }

            HStack(spacing: 10) {
                if task.openOutboxCount > 0 {
                    Button("查看待处理事项") {
                        withAnimation(.easeOut(duration: 0.2)) {
                            model.revealOutbox(for: task)
                        }
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

    private var attentionSummary: some View {
        HStack(spacing: 8) {
            Image(systemName: "person.crop.circle.badge.exclamationmark")
                .foregroundStyle(DT.amberText)
            Text(localized("该任务正在等待你处理", locale: locale))
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(DT.amberText)
            Spacer(minLength: 6)
            Button(localized("查看待处理事项", locale: locale)) {
                withAnimation(.easeOut(duration: 0.2)) {
                    model.revealOutbox(for: task)
                }
            }
            .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(DT.amberBg.opacity(0.7), in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(DT.amberStroke))
    }

    @ViewBuilder
    private var progressDetails: some View {
        if fieldVisible("taskFlow"), fieldVisible("workflow") {
            planAndWorkflowDetails
        } else {
            if fieldVisible("taskFlow") {
                planDetails
            }
            if fieldVisible("workflow") {
                workflowDetails
            }
        }
    }

    private var planAndWorkflowDetails: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 8) {
                planDetails
                    .frame(minWidth: 260)
                workflowDetails
                    .frame(minWidth: 320)
            }
            VStack(alignment: .leading, spacing: 8) {
                planDetails
                workflowDetails
            }
        }
    }

    @ViewBuilder
    private var reviewDetails: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 7) {
                Image(systemName: "checkmark.seal")
                    .foregroundStyle(DT.blueText)
                Text(localized("ActRealm Review", locale: locale))
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                if let reviewSnapshot,
                   reviewSnapshot.schemaVersion == RuntimeTaskReviewSnapshot.supportedSchemaVersion {
                    Chip(
                        text: reviewOutcomeText(reviewSnapshot.outcome),
                        tone: reviewSnapshot.outcome.state == "failed" ? .red : .neutral,
                        fontSize: 8.5
                    )
                }
                Spacer(minLength: 5)
                if reviewLoading {
                    ProgressView().controlSize(.mini)
                }
                if reviewSnapshot?.repository.state == "available" {
                    Button {
                        showingReviewDiff = true
                    } label: {
                        Image(systemName: "doc.text.magnifyingglass")
                    }
                    .buttonStyle(.plain)
                    .help(localized("查看本机 Diff", locale: locale))
                }
                Button {
                    showingCheckpoints = true
                } label: {
                    Image(systemName: "bookmark.square")
                }
                .buttonStyle(.plain)
                .help(localized("Checkpoint 与恢复", locale: locale))
                Button {
                    Task { await refreshReview(force: true) }
                } label: {
                    Image(systemName: "arrow.clockwise")
                }
                .buttonStyle(.plain)
                .help(localized("刷新 Review", locale: locale))
                .disabled(reviewLoading)
            }

            if let reviewError {
                Text(reviewError)
                    .font(.system(size: 10))
                    .foregroundStyle(DT.redText)
            } else if let reviewSnapshot,
                      reviewSnapshot.schemaVersion == RuntimeTaskReviewSnapshot.supportedSchemaVersion {
                Text(reviewRepositoryText(reviewSnapshot.repository))
                    .font(.system(size: 10.5, weight: .medium))
                    .foregroundStyle(DT.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)

                Text(reviewAttributionText(reviewSnapshot.repository))
                    .font(.system(size: 9.5))
                    .foregroundStyle(
                        ["no_changes", "exact"].contains(reviewSnapshot.repository.attribution)
                            ? DT.textWeak : DT.amberText
                    )
                    .fixedSize(horizontal: false, vertical: true)

                if let baseline = reviewBaselineText(reviewSnapshot.repository) {
                    Text(baseline)
                        .font(.system(size: 9))
                        .foregroundStyle(DT.textFaint)
                }

                if let selection = reviewRepositorySelectionText(reviewSnapshot.limitations) {
                    Text(selection)
                        .font(.system(size: 9))
                        .foregroundStyle(DT.textFaint)
                }

                if reviewSnapshot.validations.isEmpty {
                    Text(localized(
                        "当前 Turn 未观察到结构化测试或构建结果；不等于测试已通过",
                        locale: locale
                    ))
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textFaint)
                } else {
                    ForEach(reviewSnapshot.validations.suffix(4)) { validation in
                        HStack(spacing: 6) {
                            Image(systemName: reviewValidationIcon(validation.state))
                                .foregroundStyle(reviewValidationColor(validation.state))
                            Text(reviewValidationText(validation))
                                .font(.system(size: 9.5))
                                .foregroundStyle(DT.textSecondary)
                                .lineLimit(1)
                        }
                    }
                }

                if let action = reviewSnapshot.lastMeaningfulAction {
                    Text(reviewLastActionText(action))
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(1)
                }
            } else if !reviewLoading {
                Text(localized("Review 暂不可用", locale: locale))
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            }
        }
        .padding(9)
        .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(DT.neutralChipStroke, lineWidth: 1)
        )
    }

    private var reviewRefreshID: String {
        guard ProductScope.reviewEnabled, expanded else {
            return "disabled:\(task.id)"
        }
        return "expanded:\(task.id):\(task.session.execState):\(task.session.turnStartedAt ?? 0)"
    }

    @MainActor
    private func refreshReviewIfNeeded() async {
        guard expanded else { return }
        // `.task(id:)` changes whenever the current Turn identity or terminal
        // execution state changes. Discard the previous Turn's evidence before
        // fetching so a running task can never keep showing a stale completed
        // Review while the request is in flight.
        reviewSnapshot = nil
        reviewError = nil
        await refreshReview(force: true)
    }

    @MainActor
    private func refreshReview(force: Bool) async {
        guard expanded, force || reviewSnapshot == nil else { return }
        reviewLoading = true
        reviewError = nil
        defer { reviewLoading = false }
        do {
            let snapshot = try await model.client.sessionReview(sessionId: task.id)
            guard !Task.isCancelled else { return }
            reviewSnapshot = snapshot
        } catch {
            guard !Task.isCancelled else { return }
            reviewError = localized("Review 暂时无法读取", locale: locale)
        }
    }

    private func reviewOutcomeText(_ outcome: RuntimeReviewOutcome) -> String {
        switch outcome.state {
        case "completed": localized("已完成", locale: locale)
        case "failed": localized("失败", locale: locale)
        case "running": localized("运行中", locale: locale)
        default: localized("空闲", locale: locale)
        }
    }

    private func reviewRepositoryText(_ repository: RuntimeReviewRepository) -> String {
        guard repository.state == "available" else {
            return repository.state == "not_git"
                ? localized("当前工作区不是 Git 仓库", locale: locale)
                : localized("无法读取本机 Git 状态", locale: locale)
        }
        let branch = repository.branch ?? localized("detached HEAD", locale: locale)
        let head = repository.head ?? "—"
        let changed = repository.changedFiles ?? 0
        if changed == 0 {
            let clean = localizedFormat(
                "%@ · %@ · 当前工作区无未提交变更",
                locale: locale,
                branch,
                head
            )
            if let commits = repository.commitCount, commits > 0 {
                return clean + localizedFormat(
                    " · Turn 后 %lld 个 Commit",
                    locale: locale,
                    Int64(commits)
                )
            }
            return clean
        }
        let insertions = repository.insertions.map(String.init) ?? "—"
        let deletions = repository.deletions.map(String.init) ?? "—"
        let summary = localizedFormat(
            "%@ · %@ · %lld 个变更 · +%@ / -%@",
            locale: locale,
            branch,
            head,
            Int64(changed),
            insertions,
            deletions
        )
        if let commits = repository.commitCount, commits > 0 {
            return summary + localizedFormat(
                " · %lld 个 Commit",
                locale: locale,
                Int64(commits)
            )
        }
        return summary
    }

    private func reviewAttributionText(_ repository: RuntimeReviewRepository) -> String {
        switch repository.attribution {
        case "exact":
            localized("独立干净 Worktree；改动与当前 Turn 精确关联", locale: locale)
        case "bounded_window":
            switch repository.attributionReason {
            case "clean_turn_baseline":
                localized("已从当前 Turn 起点建立有界差异", locale: locale)
            case "baseline_started_dirty":
                localized("Turn 起点已有改动；只能显示有界工作区差异", locale: locale)
            case "baseline_captured_after_first_tool":
                localized("Git 基线晚于首个工具事件；不能声明完整归因", locale: locale)
            default:
                localized("已建立有界 Turn 差异", locale: locale)
            }
        case "no_changes":
            localized(
                "当前工作区干净；不等于任务没有修改，已提交结果需结合 Commit 与历史查看",
                locale: locale
            )
        case "concurrent_changes":
            localized("检测到当前 Turn 期间同一工作区存在其他任务，不能把全部改动归给当前任务", locale: locale)
        case "current_worktree_unattributed":
            localized("这是当前工作区状态；尚无 Turn 起点基线，不能把全部改动归给当前任务", locale: locale)
        default:
            localized("改动归因不可用", locale: locale)
        }
    }

    private func reviewBaselineText(_ repository: RuntimeReviewRepository) -> String? {
        guard repository.baselineState == "available",
              let capturedAt = repository.baselineCapturedAt
        else { return nil }
        let age = ZhFormat.relativeAgo(
            max(0, Date().timeIntervalSince(ZhFormat.date(fromMillis: capturedAt))),
            language: model.appLanguage
        )
        if let head = repository.baselineHead {
            return localizedFormat("Turn 起点 · %@ · %@", locale: locale, head, age)
        }
        return localizedFormat("Turn 起点 · %@", locale: locale, age)
    }

    private func reviewRepositorySelectionText(_ limitations: [String]) -> String? {
        if limitations.contains("repository_selected_by_unique_dirty_worktree") {
            return localized("仓库由唯一存在改动的子工作区识别", locale: locale)
        }
        if limitations.contains("repository_selected_as_only_nested_git") {
            return localized("仓库由唯一的子工作区识别", locale: locale)
        }
        return nil
    }

    private func reviewValidationText(_ validation: RuntimeReviewValidationRun) -> String {
        let kind = validation.kind == "test"
            ? localized("测试", locale: locale)
            : localized("构建", locale: locale)
        let state = switch validation.state {
        case "passed": localized("通过", locale: locale)
        case "failed": localized("失败", locale: locale)
        case "running": localized("运行中", locale: locale)
        default: localized("已执行，结果无法验证", locale: locale)
        }
        let tool = validation.toolName ?? localized("工具", locale: locale)
        return "\(kind) · \(tool) · \(state)"
    }

    private func reviewValidationIcon(_ state: String) -> String {
        switch state {
        case "passed": "checkmark.circle.fill"
        case "failed": "xmark.octagon.fill"
        case "running": "clock.fill"
        default: "questionmark.circle.fill"
        }
    }

    private func reviewValidationColor(_ state: String) -> Color {
        switch state {
        case "passed": DT.greenText
        case "failed": DT.redText
        case "running": DT.amberText
        default: DT.textWeak
        }
    }

    private func reviewLastActionText(_ action: RuntimeReviewLastAction) -> String {
        let tool = action.toolName.map { " · \($0)" } ?? ""
        return localizedFormat(
            "最后有效动作 · %@%@",
            locale: locale,
            action.kind,
            tool
        )
    }

    private var planDetails: some View {
        let summary = task.planProgress.map {
            TaskPlanPresentation.summary(
                done: $0.done,
                total: $0.total,
                steps: task.session.planSteps
            )
        }
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text(localized("任务流程", locale: locale))
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(DT.textWeak)
                Spacer(minLength: 4)
                if let summary {
                    Text(TaskPlanPresentation.summaryText(
                        summary,
                        locale: locale
                    ))
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundStyle(DT.textFaint)
                        .lineLimit(1)
                }
            }
            if task.session.planSteps.isEmpty {
                Text(planText)
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            } else {
                Text(TaskPlanPresentation.sourceText(
                    provider: provider,
                    steps: task.session.planSteps,
                    locale: locale
                ))
                    .font(.system(size: 9))
                    .foregroundStyle(DT.textFaint)
                    .lineLimit(1)
                ScrollView(.vertical, showsIndicators: true) {
                    LazyVStack(alignment: .leading, spacing: 6) {
                        ForEach(task.session.planSteps.prefix(64)) { step in
                            HStack(alignment: .top, spacing: 7) {
                                Image(systemName: planIcon(for: step.status))
                                    .font(.system(size: 10, weight: .semibold))
                                    .foregroundStyle(planColor(for: step.status))
                                    .frame(width: 12, height: 15)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(step.text)
                                        .font(.system(size: 10.5, weight: step.status == "in_progress" ? .semibold : .regular))
                                        .foregroundStyle(DT.textSecondary)
                                        .lineLimit(2)
                                    if let detail = step.detail, !detail.isEmpty {
                                        Text(detail)
                                            .font(.system(size: 9.5))
                                            .foregroundStyle(DT.textFaint)
                                            .lineLimit(2)
                                    }
                                }
                            }
                        }
                    }
                }
                .scrollBounceBehavior(.basedOnSize)
                .frame(
                    height: TaskPlanPresentation.panelHeight(
                        stepCount: task.session.planSteps.count
                    )
                )
            }
        }
        .padding(9)
        .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(DT.neutralChipStroke, lineWidth: 1)
        )
    }

    private var workflowDetails: some View {
        let items = TaskWorkflowPresentation.items(
            model.isDemo ? DemoData.timeline(sessionID: task.id, now: model.now) : workflowEvents,
            activeToolName: task.session.execState == "tool_running"
                ? task.session.currentTool
                : nil
        )
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text(localized("工作流", locale: locale))
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(DT.textWeak)
                if workflowLoading {
                    ProgressView()
                        .controlSize(.mini)
                }
                Spacer(minLength: 4)
                TaskWorkflowFreshnessLabel(
                    lastEventAt: task.lastEventAt,
                    running: task.status == .running,
                    language: model.appLanguage,
                    locale: locale
                )
            }
            if let workflowError {
                Text(workflowError)
                    .font(.system(size: 10))
                    .foregroundStyle(DT.redText)
            }
            if items.isEmpty && !workflowLoading && workflowError == nil {
                Text(TaskWorkflowPresentation.emptyText(
                    capability: model.client.snapshot.providerCapability(
                        for: provider,
                        feature: .toolLifecycle
                    ).status,
                    running: task.status == .running,
                    locale: locale
                ))
                    .font(.system(size: 10))
                    .foregroundStyle(DT.textFaint)
            } else if !items.isEmpty {
                if let workflowEarlierError {
                    Text(workflowEarlierError)
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.redText)
                }
                TaskWorkflowEventList(
                    items: items,
                    hasEarlierEvents: workflowHasMore,
                    reachedDisplayLimit: workflowReachedDisplayLimit,
                    loadingEarlier: workflowLoadingEarlier,
                    onLoadEarlier: { Task { await loadEarlierWorkflow() } },
                    locale: locale
                )
            }
        }
        .padding(9)
        .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(DT.neutralChipStroke, lineWidth: 1)
        )
    }

    private var workflowRefreshID: String {
        guard expanded, fieldVisible("workflow") else {
            return "collapsed:\(task.id)"
        }
        return "expanded:\(task.id):\(task.session.lastEventAt)"
    }

    @MainActor
    private func refreshWorkflowIfNeeded() async {
        if model.isDemo { return }
        guard expanded, fieldVisible("workflow") else { return }
        workflowLoading = workflowEvents.isEmpty
        workflowError = nil
        workflowEarlierError = nil
        do {
            let page = try await model.client.sessionActivity(
                sessionId: task.id,
                limit: 100
            )
            guard !Task.isCancelled else { return }
            workflowEvents = page.events
            workflowHasMore = page.hasMore
            workflowReachedDisplayLimit = false
        } catch {
            guard !Task.isCancelled else { return }
            workflowError = localized("工作流暂时无法读取", locale: locale)
        }
        workflowLoading = false
    }

    @MainActor
    private func loadEarlierWorkflow() async {
        guard !workflowLoadingEarlier,
              workflowEvents.count < TaskWorkflowPresentation.maximumVisibleEvents,
              let firstSequence = workflowEvents.first?.ingestSequence
        else { return }
        workflowLoadingEarlier = true
        workflowEarlierError = nil
        defer { workflowLoadingEarlier = false }
        do {
            let page = try await model.client.sessionActivity(
                sessionId: task.id,
                limit: 100,
                beforeIngestSequence: firstSequence
            )
            guard !Task.isCancelled else { return }
            let existingIDs = Set(workflowEvents.map(\.eventId))
            let earlier = page.events.filter { !existingIDs.contains($0.eventId) }
            let available = max(
                0,
                TaskWorkflowPresentation.maximumVisibleEvents - workflowEvents.count
            )
            workflowEvents = Array(earlier.suffix(available)) + workflowEvents
            workflowReachedDisplayLimit = page.hasMore
                && workflowEvents.count >= TaskWorkflowPresentation.maximumVisibleEvents
            workflowHasMore = page.hasMore && !workflowReachedDisplayLimit
        } catch {
            guard !Task.isCancelled else { return }
            workflowEarlierError = localized("较早工作流暂时无法读取", locale: locale)
        }
    }

    private var subagentDetails: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 6) {
                Text("运行中的子 Agent")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(DT.textWeak)
                ForEach(task.session.subagents.prefix(8)) { agent in
                    Image(systemName: "sparkles")
                        .font(.system(size: 8, weight: .semibold))
                        .foregroundStyle(DT.blueText)
                        .frame(width: 18, height: 18)
                        .background(DT.blueBg, in: Circle())
                        .help(agent.agentType ?? localized("子 Agent", locale: locale))
                }
            }
            ForEach(task.session.subagents.prefix(6)) { agent in
                HStack(spacing: 6) {
                    Circle().fill(DT.greenDot).frame(width: 5, height: 5)
                    Text(agent.agentType ?? localized("子 Agent", locale: locale))
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(DT.textSecondary)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(subagentStatus(agent.status))
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textFaint)
                }
            }
        }
        .padding(9)
        .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(DT.neutralChipStroke, lineWidth: 1)
        )
    }

    private func subagentStatus(_ status: String) -> String {
        let key = switch status {
        case "pendingInit": "准备中"
        case "running", "started", "interacted": "运行中"
        default: status
        }
        return localized(key, locale: locale)
    }

    private func planIcon(for status: String) -> String {
        switch status {
        case "completed": "checkmark.circle.fill"
        case "in_progress": "circle.dotted"
        default: "circle"
        }
    }

    private func planColor(for status: String) -> Color {
        switch status {
        case "completed": DT.greenText
        case "in_progress": DT.blue
        default: DT.textFaint
        }
    }

    @ViewBuilder
    private var usageStrip: some View {
        if ((fieldVisible("sessionTokens") && task.totalTokens != nil)
                || (!usageIsProvisional && fieldVisible("cost") && task.estimatedCostUsdMicros != nil))
            || (fieldVisible("context") && task.contextUsageFraction != nil) {
            HStack(spacing: 6) {
                if fieldVisible("sessionTokens"),
                   let total = task.totalTokens {
                    usageChip(localizedFormat(
                        usageIsProvisional ? "已观测 %@ Token" : "累计 %@ Token",
                        locale: locale,
                        ZhFormat.tokenCount(total)
                    ), tone: .neutral)
                }
                if fieldVisible("context"), let fraction = task.contextUsageFraction {
                    usageChip(localizedFormat(
                        "上下文 %lld%%",
                        locale: locale,
                        Int64((fraction * 100).rounded())
                    ), tone: fraction >= 0.7 ? .amber : .blue)
                }
                if !usageIsProvisional,
                   fieldVisible("cost"), task.estimatedCostUsdMicros != nil {
                    usageChip(localizedFormat(
                        "API 等价值 %@",
                        locale: locale,
                        estimatedCostText
                    ), tone: .blue)
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
        case .custom: return "Provider 连接器"
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
        if let plan = task.planProgress {
            return TaskPlanPresentation.compactText(
                done: plan.done,
                total: plan.total,
                steps: task.session.planSteps,
                locale: locale
            )
        }
        switch model.client.snapshot.providerCapability(
            for: provider,
            feature: .plan
        ).status {
        case .supported:
            return localized(
                task.status == .running
                    ? "当前 Turn 尚未收到计划事件"
                    : "当前 Turn 已结束，未提供计划",
                locale: locale
            )
        case .unsupported:
            return localized("Provider 不支持计划事件", locale: locale)
        case .unknown:
            return localized("Provider 计划能力尚未确认", locale: locale)
        }
    }
    private var subagentText: String {
        if let count = task.session.activeSubagents, count > 0 {
            return "\(count)"
        }
        switch model.client.snapshot.providerCapability(
            for: provider,
            feature: .subagents
        ).status {
        case .supported:
            return localized("暂无活动子 Agent", locale: locale)
        case .unsupported:
            return localized("Provider 不支持子 Agent 状态", locale: locale)
        case .unknown:
            return localized("Provider 子 Agent 能力尚未确认", locale: locale)
        }
    }
    private var tokenText: String {
        guard let total = task.totalTokens else { return localized("暂无数据", locale: locale) }
        if usageIsProvisional {
            return localizedFormat(
                "已观测 %@ · 尚未完成核对",
                locale: locale,
                ZhFormat.tokenCount(total)
            )
        }
        return ZhFormat.tokenCount(total)
    }
    private var tokenIsHigh: Bool { !usageIsProvisional && (task.totalTokens ?? 0) >= 140_000 }
    private var lastTurnTokenText: String { task.lastTurnTokens.map(ZhFormat.tokenCount) ?? "—" }
    private var usageFactText: String {
        AppLocalization.localizedUsageDescription(source: task.session.usageSource,
            quality: task.session.usageQuality, language: model.appLanguage)
    }
    private var estimatedCostText: String {
        if usageIsProvisional {
            return localized("暂不显示（Token 数据尚未完成核对）", locale: locale)
        }
        guard let micros = task.estimatedCostUsdMicros else { return "—" }
        let dollars = Double(micros) / 1_000_000
        return dollars > 0 && dollars < 0.01
            ? String(format: "$%.4f", dollars)
            : String(format: "$%.2f", dollars)
    }
    private var usageIsProvisional: Bool { task.usageIsProvisional }
    private var recoveryText: String {
        let key = switch task.recoveryPresentation {
        case .controllable: "已重新连接，可控制"
        case .observing: "仍在运行，仅可观察"
        case .waitingForEvent: "历史已恢复，等待新事件"
        case .lostControl: "已失去控制"
        case .ended: "已结束"
        case .unknown: "等待确认状态"
        }
        return localized(key, locale: locale)
    }
    private var controlText: String {
        guard task.session.controlCapability == "managed" else {
            return localized("外部 Hook，仅观察 / 授权", locale: locale)
        }
        let directRequestOpen = model.derived.openOutbox.contains {
            $0.attention.sessionId == task.id
                && $0.kind == .approval
                && $0.attention.requestId != nil
        }
        if directRequestOpen {
            return localized("托管请求已接入，可直接审批", locale: locale)
        }
        return localized(model.client.snapshot.capabilities?.codexConnector?.managedApprovals == true
            ? "app-server 已连接；原生审批仍需在 Codex 处理"
            : "app-server 已连接；当前版本审批需原界面", locale: locale)
    }
    private var metaLine: String {
        var parts = [providerName]
        if fieldVisible("project") {
            parts.append(task.projectName.flatMap { $0.isEmpty ? nil : $0 }
                ?? localized("项目未知", locale: locale))
        }
        if fieldVisible("model") {
            parts.append(task.model ?? localized("模型未知", locale: locale))
        }
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
        if fieldVisible("sessionTokens") {
            items.append(("会话累计 Token", tokenText, tokenIsHigh))
        }
        if fieldVisible("turnTokens") {
            items.append((task.session.provider == "codex" ? "最近调用 Token" : "本轮 Token", lastTurnTokenText, false))
        }
        if fieldVisible("sessionTokens") {
            items.append(("Token 数据", usageFactText, false))
        }
        if fieldVisible("inputOutputTokens") {
            if task.inputTokens != nil || task.outputTokens != nil {
                items.append((
                    "输入 / 输出",
                    "\(task.inputTokens.map(ZhFormat.tokenCount) ?? "—") / \(task.outputTokens.map(ZhFormat.tokenCount) ?? "—")",
                    false
                ))
            }
        }
        if fieldVisible("cacheTokens") {
            if task.session.cacheReadTokens != nil || task.session.cacheCreationTokens != nil {
                items.append((
                    "缓存读取 / 写入",
                    "\(task.session.cacheReadTokens.map(ZhFormat.tokenCount) ?? "—") / \(task.session.cacheCreationTokens.map(ZhFormat.tokenCount) ?? "—")",
                    false
                ))
            }
        }
        if fieldVisible("reasoningTokens") {
            if let reasoning = task.session.reasoningTokens {
                items.append(("推理 Token", ZhFormat.tokenCount(reasoning), false))
            }
        }
        if !usageIsProvisional, fieldVisible("cost") {
            items.append(("API 等价值", estimatedCostText, false))
        }
        if fieldVisible("tool") {
            items.append(("当前动作", currentActionText, false))
        }
        if fieldVisible("currentTarget") {
            items.append(("当前文件 / 目标", currentTargetText, false))
        }
        if fieldVisible("permissionMode") { items.append(("权限模式", task.session.permissionMode ?? "—", false)) }
        if fieldVisible("subagents") {
            items.append(("运行中的子 Agent", subagentText, false))
        }
        if fieldVisible("recovery") { items.append(("恢复状态", recoveryText, false)) }
        if fieldVisible("control") { items.append(("托管能力", controlText, false)) }
        if fieldVisible("jump") { items.append(("打开应用", task.session.jumpCapability == "unsupported" ? "当前环境不支持" : "可用", false)) }
        if fieldVisible("titleSource") { items.append(("标题来源", titleSourceText, false)) }
        if fieldVisible("sessionId") { items.append(("ActRealm Session ID", task.id, false)) }
        if fieldVisible("providerSessionId") { items.append(("Provider Session ID", task.session.providerSessionId, false)) }
        if fieldVisible("providerTurnId") { items.append(("Provider Turn ID", task.session.providerTurnId ?? "—", false)) }
        if fieldVisible("lastEventAt") { items.append(("最后事件", ZhFormat.timestamp(task.lastEventAt, language: model.appLanguage), false)) }
        return items
    }
    private func primaryDetailItems(
        from items: [(label: String, value: String, emphasized: Bool)]
    ) -> [(label: String, value: String, emphasized: Bool)] {
        let primaryLabels: Set<String> = [
            "工作区",
            "本轮上下文",
            "当前动作",
            "当前文件 / 目标",
            "恢复状态",
            "托管能力",
        ]
        return items.filter { primaryLabels.contains($0.label) }
    }
    private func secondaryDetailItems(
        from items: [(label: String, value: String, emphasized: Bool)]
    ) -> [(label: String, value: String, emphasized: Bool)] {
        let primaryLabels = Set(primaryDetailItems(from: items).map(\.label))
        return items.filter { !primaryLabels.contains($0.label) }
    }
    private var reviewHasEvidence: Bool {
        guard let reviewSnapshot,
              reviewSnapshot.schemaVersion == RuntimeTaskReviewSnapshot.supportedSchemaVersion
        else { return false }
        return (reviewSnapshot.repository.changedFiles ?? 0) > 0
            || !reviewSnapshot.validations.isEmpty
            || reviewSnapshot.outcome.state == "failed"
    }
    private var shouldShowReview: Bool {
        guard ProductScope.reviewEnabled else { return false }
        return task.status != .running || reviewHasEvidence
    }
    private var currentActionText: String {
        task.localizedCurrentAction(language: model.appLanguage)
    }
    private var currentTargetText: String {
        if let target = task.session.currentTarget, !target.isEmpty {
            return target
        }
        let capability = model.client.snapshot.providerCapability(
            for: provider,
            feature: .currentTarget
        ).status
        switch capability {
        case .unsupported:
            return localized("Provider 不提供文件目标", locale: locale)
        case .unknown:
            return localized("暂无可靠文件信息", locale: locale)
        case .supported:
            return localized(
                task.session.execState == "tool_running"
                    ? "当前工具没有文件目标"
                    : "当前阶段没有文件目标",
                locale: locale
            )
        }
    }
    private var titleSourceText: String {
        switch task.session.providerTitleSource {
        case "codex_thread_name": localized("Codex 会话名称", locale: locale)
        case "claude_custom_title": localized("Claude 自定义标题", locale: locale)
        case "claude_session_title": localized("Claude 会话标题", locale: locale)
        case "claude_ai_title": localized("Claude 生成标题", locale: locale)
        case .some(let source): source
        case nil: localized("安全任务摘要 / 项目回退", locale: locale)
        }
    }
    private var note: String {
        let key = switch task.status {
        case .done: "确认后归档本轮"
        case .idle: "最近没有新的活动"
        default: provider == .codex
            ? "状态粒度由当前 Hook / Connector 能力决定"
            : "只显示 Runtime 已验证的工具与计划事件"
        }
        return localized(key, locale: locale)
    }
    private var providerName: String {
        provider.displayName
    }
    private var badge: String {
        task.localizedState(language: model.appLanguage)
    }
    private var rightColor: Color {
        switch task.status {
        case .waiting:
            return DT.amberText
        case .running:
            return DT.blueText
        case .failed:
            return DT.redText
        case .done:
            return DT.textSecondary
        case .idle:
            return DT.textWeak
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
        return switch task.status {
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

enum TaskPlanPresentation {
    struct Summary: Equatable {
        let currentStep: Int?
        let completed: Int
        let inProgress: Int
        let pending: Int
        let total: Int
    }

    static func summary(
        done: Int,
        total: Int,
        steps: [PlanStepRecord]
    ) -> Summary {
        let boundedTotal = max(total, steps.count)
        let completed = min(max(done, 0), boundedTotal)
        let inProgress = steps.filter { $0.status == "in_progress" }.count
        let currentStep = steps.firstIndex { $0.status == "in_progress" }.map { $0 + 1 }
        let pending = max(0, boundedTotal - completed - inProgress)
        return Summary(
            currentStep: currentStep,
            completed: completed,
            inProgress: inProgress,
            pending: pending,
            total: boundedTotal
        )
    }

    static func compactText(
        done: Int,
        total: Int,
        steps: [PlanStepRecord],
        locale: Locale
    ) -> String {
        let summary = summary(done: done, total: total, steps: steps)
        if summary.completed >= summary.total {
            return localizedFormat(
                "%lld/%lld（已完成）",
                locale: locale,
                Int64(summary.completed),
                Int64(summary.total)
            )
        }
        if let currentStep = summary.currentStep {
            return localizedFormat(
                "当前第 %lld/%lld 步 · 已完成 %lld",
                locale: locale,
                Int64(currentStep),
                Int64(summary.total),
                Int64(summary.completed)
            )
        }
        return progressText(
            done: summary.completed,
            total: summary.total,
            locale: locale
        )
    }

    static func summaryText(_ summary: Summary, locale: Locale) -> String {
        localizedFormat(
            "已完成 %lld · 进行中 %lld · 待处理 %lld",
            locale: locale,
            Int64(summary.completed),
            Int64(summary.inProgress),
            Int64(summary.pending)
        )
    }

    static func sourceText(
        provider: ProviderKind,
        steps: [PlanStepRecord],
        locale: Locale
    ) -> String {
        let source = steps.first?.source ?? ""
        let providerName = provider.displayName
        let kind = source == "claude_task"
            ? localized("Task 事件", locale: locale)
            : localized("结构化计划事件", locale: locale)
        return localizedFormat(
            "来源 %@ · %@ · 随 Provider 更新",
            locale: locale,
            providerName,
            kind
        )
    }

    static func panelHeight(stepCount: Int) -> CGFloat {
        min(220, max(54, CGFloat(stepCount) * 34))
    }

    static func progressText(
        done: Int,
        total: Int,
        locale: Locale
    ) -> String {
        localizedFormat(
            done >= total ? "%lld/%lld（已完成）" : "%lld/%lld（进行中）",
            locale: locale,
            Int64(done),
            Int64(total)
        )
    }
}

enum TaskWorkflowPresentation {
    struct Item: Identifiable, Equatable {
        let id: String
        let event: RuntimeTimelineEvent
        let startedAt: UInt64?
        let toolTarget: String?
        let isLive: Bool
        let occurrenceCount: Int
        let accumulatedDurationMillis: UInt64?
        var startedEvent: RuntimeTimelineEvent? = nil

        var toolCategory: String? {
            let terminal = event.toolCategory
            if let start = startedEvent?.toolCategory, !["tool", "shell"].contains(start),
               terminal == nil || ["tool", "shell"].contains(terminal ?? "") { return start }
            return terminal ?? startedEvent?.toolCategory
        }
        var toolName: String? { event.toolName ?? startedEvent?.toolName }

        var durationMillis: UInt64? {
            if let accumulatedDurationMillis { return accumulatedDurationMillis }
            guard let startedAt, event.occurredAt >= startedAt else { return nil }
            return event.occurredAt - startedAt
        }
    }

    static let maximumVisibleEvents = 1_000
    static let focusedEventLimit = 3

    static func focusedItems(_ items: [Item]) -> [Item] {
        items.filter(isMeaningfulStatus).suffix(focusedEventLimit)
    }

    private static func isMeaningfulStatus(_ item: Item) -> Bool {
        if isFailure(item) || matchesValidationCategory(item.toolCategory) {
            return true
        }
        if item.event.kind.hasPrefix("tool.") {
            return item.toolCategory == "file_edit"
        }
        return [
            "turn.completed", "turn.interrupted", "turn.failed",
            "approval.requested", "approval.resolved",
            "question.requested", "elicitation.requested",
            "task.created", "task.completed", "plan.updated",
            "subagent.started", "subagent.completed",
        ].contains(item.event.kind)
    }

    static func visibleEvents(
        _ events: [RuntimeTimelineEvent]
    ) -> [RuntimeTimelineEvent] {
        Array(events.suffix(maximumVisibleEvents))
    }

    static func items(
        _ events: [RuntimeTimelineEvent],
        activeToolName: String? = nil
    ) -> [Item] {
        var result: [Item] = []
        for event in visibleEvents(events) {
            if event.kind == "tool.started" {
                result.append(Item(
                    id: event.eventId,
                    event: event,
                    startedAt: event.occurredAt,
                    toolTarget: event.toolTarget,
                    isLive: false,
                    occurrenceCount: 1,
                    accumulatedDurationMillis: nil
                ))
                continue
            }
            if event.kind == "tool.completed" || event.kind == "tool.failed",
               let startIndex = result.lastIndex(where: {
                   matchingToolStart($0.event, terminal: event)
               }) {
                let start = result[startIndex]
                result[startIndex] = Item(
                    id: start.id,
                    event: event,
                    startedAt: start.startedAt,
                    toolTarget: start.toolTarget ?? event.toolTarget,
                    isLive: false,
                    occurrenceCount: 1,
                    accumulatedDurationMillis: nil,
                    startedEvent: start.event
                )
                continue
            }
            result.append(Item(
                id: event.eventId,
                event: event,
                startedAt: nil,
                toolTarget: event.toolTarget,
                isLive: false,
                occurrenceCount: 1,
                accumulatedDurationMillis: nil
            ))
        }
        if let activeToolName,
           let activeIndex = result.lastIndex(where: {
               $0.event.kind == "tool.started"
                   && normalizedToolName($0.event.toolName) == normalizedToolName(activeToolName)
           }) {
            let active = result[activeIndex]
            result[activeIndex] = Item(
                id: active.id,
                event: active.event,
                startedAt: active.startedAt,
                toolTarget: active.toolTarget,
                isLive: true,
                occurrenceCount: 1,
                accumulatedDurationMillis: nil
            )
        }
        return collapseRoutineShellRows(result)
    }

    /// A stream of short successful shell calls is implementation noise, but
    /// an active, failed, or long-running command is operationally useful.
    /// Collapse only adjacent routine successes and preserve everything that
    /// may need the developer's attention as its own row.
    private static func collapseRoutineShellRows(_ items: [Item]) -> [Item] {
        var collapsed: [Item] = []
        var index = 0
        while index < items.count {
            let first = items[index]
            guard isRoutineShellSuccess(first) else {
                collapsed.append(first)
                index += 1
                continue
            }
            var group = [first]
            var cursor = index + 1
            while cursor < items.count,
                  isRoutineShellSuccess(items[cursor]),
                  items[cursor].event.turnId == first.event.turnId,
                  groupingCategory(items[cursor]) == groupingCategory(first)
            {
                group.append(items[cursor])
                cursor += 1
            }
            guard group.count > 1, let last = group.last else {
                collapsed.append(first)
                index += 1
                continue
            }
            let durations = group.compactMap(\.durationMillis)
            collapsed.append(Item(
                id: "shell-group:\(first.id):\(last.id)",
                event: last.event,
                startedAt: nil,
                toolTarget: nil,
                isLive: false,
                occurrenceCount: group.count,
                accumulatedDurationMillis: durations.isEmpty
                    ? nil
                    : durations.reduce(0, &+),
                startedEvent: first.startedEvent
            ))
            index = cursor
        }
        return collapsed
    }

    private static func isRoutineShellSuccess(_ item: Item) -> Bool {
        guard item.event.kind == "tool.completed", !item.isLive, !isFailure(item),
              item.toolTarget == nil,
              item.toolCategory == nil || ["shell", "tool"].contains(item.toolCategory ?? "")
        else { return false }
        guard let name = item.event.toolName?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        else { return false }
        guard ["bash", "shell", "exec_command"].contains(name) else { return false }
        return item.durationMillis.map { $0 < 10_000 } ?? true
    }

    private static func normalizedToolName(_ name: String?) -> String? {
        guard let name, name != "Unknown" else { return nil }
        return name
    }

    private static func groupingCategory(_ item: Item) -> String {
        item.toolCategory ?? item.event.toolName ?? "tool"
    }

    private static func matchingToolStart(
        _ start: RuntimeTimelineEvent,
        terminal: RuntimeTimelineEvent
    ) -> Bool {
        guard start.kind == "tool.started" else { return false }
        if start.toolCallId != nil || terminal.toolCallId != nil {
            guard start.toolCallId == terminal.toolCallId else { return false }
        } else if start.toolName != terminal.toolName {
            return false
        }
        if let startTurn = start.turnId, let terminalTurn = terminal.turnId {
            return startTurn == terminalTurn
        }
        return true
    }

    static func emptyText(
        capability: ProviderCapabilityStatus,
        running: Bool,
        locale: Locale
    ) -> String {
        switch capability {
        case .unsupported:
            localized("Provider 不支持工具工作流", locale: locale)
        case .unknown:
            localized("Provider 工具工作流能力尚未确认", locale: locale)
        case .supported where running:
            localized("等待当前 Turn 的首个工具事件", locale: locale)
        case .supported:
            localized("当前 Turn 没有工具调用", locale: locale)
        }
    }

    static func title(for item: Item, locale: Locale) -> String {
        guard item.event.kind.hasPrefix("tool.") else {
            return title(for: item.event, locale: locale)
        }
        let language: AppLanguage = AppLanguage.resolvedIdentifier(for: locale) == "zh-Hans" ? .simplifiedChinese : .english
        let operation = TaskActivityPresentation.action(category: item.toolCategory,
            tool: item.toolName, running: false, language: language)
        let count = item.occurrenceCount > 1 ? " × \(item.occurrenceCount)" : ""
        let target = item.toolTarget.map { " · \($0)" } ?? ""
        let state = validationState(for: item, locale: locale)
        return "\(operation)\(count)\(target) · \(state)"
    }

    private static func validationState(for item: Item, locale: Locale) -> String {
        if matchesValidationCategory(item.toolCategory) {
            if item.event.kind == "tool.started" {
                return localized(item.isLive ? "执行中" : "未收到结束事件", locale: locale)
            }
            switch item.event.validationStatus {
            case "passed": return localized("通过", locale: locale)
            case "failed": return localized("失败", locale: locale)
            case "unverifiable", nil:
                return localized("已执行，结果无法验证", locale: locale)
            default: return localized("已执行，结果无法验证", locale: locale)
            }
        }
        return switch item.event.kind {
        case "tool.started": item.isLive
            ? localized("执行中", locale: locale)
            : localized("未收到结束事件", locale: locale)
        case "tool.failed": localized("失败", locale: locale)
        default: localized("已完成", locale: locale)
        }
    }

    private static func matchesValidationCategory(_ category: String?) -> Bool {
        category == "test" || category == "build" || category == "code_check"
    }

    static func categoryLabel(_ category: String?, locale: Locale) -> String? {
        switch category {
        case "code_check": localized("代码检查", locale: locale)
        case "test": localized("测试", locale: locale)
        case "build": localized("构建", locale: locale)
        case "version_control": localized("版本控制", locale: locale)
        case "package": localized("依赖管理", locale: locale)
        case "network": localized("网络访问", locale: locale)
        case "file_edit": localized("文件编辑", locale: locale)
        case "file_read": localized("文件读取", locale: locale)
        case "file_search": localized("文件查询", locale: locale)
        case "process": localized("后台进程", locale: locale)
        case "code_execution": localized("代码执行", locale: locale)
        case "interaction": localized("界面交互", locale: locale)
        case "shell": localized("Shell 操作", locale: locale)
        case "tool": nil
        default: nil
        }
    }

    static func currentActionLabel(_ category: String?, locale: Locale) -> String {
        TaskActivityPresentation.action(category: category, tool: nil, running: true,
            language: AppLanguage.resolvedIdentifier(for: locale) == "zh-Hans" ? .simplifiedChinese : .english)
    }

    static func durationText(for item: Item, locale: Locale) -> String? {
        guard let durationMillis = item.durationMillis,
              item.event.kind != "tool.started"
        else { return nil }
        let seconds = Double(durationMillis) / 1_000
        if seconds < 60 {
            return String(format: "%.1f %@", seconds, localized("秒", locale: locale))
        }
        let language: AppLanguage = AppLanguage.resolvedIdentifier(for: locale) == "zh-Hans"
            ? .simplifiedChinese
            : .english
        return ZhFormat.waitDuration(seconds, language: language)
    }

    static func panelHeight(itemCount: Int) -> CGFloat {
        min(220, max(48, CGFloat(itemCount) * 46))
    }

    static func title(
        for event: RuntimeTimelineEvent,
        locale: Locale
    ) -> String {
        switch event.kind {
        case "session.started":
            localized("会话已连接", locale: locale)
        case "session.ended":
            localized("会话已结束", locale: locale)
        case "session.compacting":
            localized("正在压缩上下文", locale: locale)
        case "turn.started":
            localized("开始处理任务", locale: locale)
        case "turn.completed":
            localized("本轮任务已完成", locale: locale)
        case "turn.interrupted":
            localized("本轮任务已中断", locale: locale)
        case "turn.failed":
            localized("本轮任务运行失败", locale: locale)
        case "tool.started":
            if let toolName = event.toolName {
                localizedFormat("开始使用 %@", locale: locale, toolName)
            } else {
                localized("开始使用工具", locale: locale)
            }
        case "tool.completed":
            if let toolName = event.toolName {
                localizedFormat("完成 %@", locale: locale, toolName)
            } else {
                localized("工具执行完成", locale: locale)
            }
        case "tool.failed":
            if let toolName = event.toolName {
                localizedFormat("%@ 运行失败", locale: locale, toolName)
            } else {
                localized("工具运行失败", locale: locale)
            }
        case "approval.requested":
            localized("请求用户批准", locale: locale)
        case "approval.resolved":
            localized("批准请求已处理", locale: locale)
        case "question.requested":
            localized("等待用户回答", locale: locale)
        case "elicitation.requested":
            localized("请求用户补充信息", locale: locale)
        case "subagent.started":
            localized("子 Agent 已启动", locale: locale)
        case "subagent.completed":
            localized("子 Agent 已完成", locale: locale)
        case "task.created":
            localized("新增任务步骤", locale: locale)
        case "task.completed":
            localized("任务步骤已完成", locale: locale)
        case "plan.updated":
            if let count = event.planStepCount {
                localizedFormat(
                    "任务流程已更新 · %lld 个步骤",
                    locale: locale,
                    Int64(count)
                )
            } else {
                localized("任务流程已更新", locale: locale)
            }
        default:
            localized("Agent 活动", locale: locale)
        }
    }

    static func icon(for kind: String) -> String {
        switch kind {
        case "session.started": "link.circle.fill"
        case "session.ended": "stop.circle"
        case "session.compacting": "arrow.triangle.2.circlepath"
        case "turn.started": "play.circle.fill"
        case "turn.completed": "checkmark.circle.fill"
        case "turn.interrupted": "pause.circle.fill"
        case "turn.failed", "tool.failed": "exclamationmark.triangle.fill"
        case "tool.started": "hammer.fill"
        case "tool.completed": "checkmark.circle"
        case "approval.requested", "question.requested", "elicitation.requested":
            "person.crop.circle.badge.questionmark"
        case "approval.resolved": "person.crop.circle.badge.checkmark"
        case "subagent.started", "subagent.completed": "sparkles"
        case "task.created", "task.completed", "plan.updated": "checklist"
        default: "waveform.path.ecg"
        }
    }

    static func icon(for item: Item) -> String {
        if matchesValidationCategory(item.toolCategory) {
            switch item.event.validationStatus {
            case "failed": return "exclamationmark.triangle.fill"
            case "unverifiable", nil: return "questionmark.circle.fill"
            default: break
            }
        }
        if item.event.kind == "tool.started" {
            switch TaskActivityPresentation.category(item.toolCategory, tool: item.toolName) {
            case "file_read", "file_search": return "doc.text.magnifyingglass"
            case "file_edit": return "pencil"
            case "test", "code_check": return "checkmark.seal"
            case "network": return "network"
            case "interaction": return "cursorarrow.click"
            case "code_execution", "shell", "process": return "terminal"
            default: break
            }
        }
        return icon(for: item.event.kind)
    }

    static func isFailure(_ kind: String) -> Bool {
        kind == "turn.failed" || kind == "tool.failed"
    }

    static func isFailure(_ item: Item) -> Bool {
        item.event.validationStatus == "failed" || isFailure(item.event.kind)
    }

    static func time(
        for event: RuntimeTimelineEvent,
        locale: Locale
    ) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.dateFormat = "HH:mm:ss"
        return formatter.string(
            from: ZhFormat.date(fromMillis: event.occurredAt)
        )
    }
}

private struct TaskWorkflowEventList: View {
    let items: [TaskWorkflowPresentation.Item]
    let hasEarlierEvents: Bool
    let reachedDisplayLimit: Bool
    let loadingEarlier: Bool
    let onLoadEarlier: () -> Void
    let locale: Locale
    @State private var visibleBottomID: String?
    @State private var unseenCount = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            if hasEarlierEvents {
                HStack(spacing: 6) {
                    Text(localizedFormat(
                        "当前已载入 %lld 条",
                        locale: locale,
                        Int64(items.count)
                    ))
                        .font(.system(size: 9))
                        .foregroundStyle(DT.textFaint)
                    Button(action: onLoadEarlier) {
                        if loadingEarlier {
                            ProgressView().controlSize(.mini)
                        } else {
                            Text(localized("加载更早事件", locale: locale))
                        }
                    }
                    .buttonStyle(.plain)
                    .font(.system(size: 9, weight: .medium))
                    .foregroundStyle(DT.blueText)
                    .disabled(loadingEarlier)
                }
            } else if reachedDisplayLimit {
                Text(localizedFormat(
                    "已达到 %lld 条显示上限",
                    locale: locale,
                    Int64(TaskWorkflowPresentation.maximumVisibleEvents)
                ))
                    .font(.system(size: 9))
                    .foregroundStyle(DT.textFaint)
            }
            ZStack(alignment: .topTrailing) {
                ScrollView(.vertical, showsIndicators: true) {
                    LazyVStack(alignment: .leading, spacing: 5) {
                        ForEach(items) { item in
                            workflowRow(item)
                                .id(item.id)
                        }
                    }
                    .scrollTargetLayout()
                }
                .scrollPosition(id: $visibleBottomID, anchor: .bottom)
                .defaultScrollAnchor(.bottom)
                .scrollBounceBehavior(.basedOnSize)

                if unseenCount > 0 {
                    Button(localizedFormat(
                        "%lld 条新事件",
                        locale: locale,
                        Int64(unseenCount)
                    )) {
                        withAnimation(.easeOut(duration: 0.18)) {
                            visibleBottomID = items.last?.id
                            unseenCount = 0
                        }
                    }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                    .padding(5)
                }
            }
            .frame(height: TaskWorkflowPresentation.panelHeight(itemCount: items.count))
        }
        .onAppear {
            visibleBottomID = items.last?.id
        }
        .onChange(of: items.last?.id) { oldValue, newValue in
            guard let newValue else { return }
            if visibleBottomID == nil || visibleBottomID == oldValue {
                visibleBottomID = newValue
                unseenCount = 0
            } else {
                unseenCount += 1
            }
        }
        .onChange(of: visibleBottomID) { _, newValue in
            if newValue == items.last?.id {
                unseenCount = 0
            }
        }
    }

    @ViewBuilder
    private func workflowRow(_ item: TaskWorkflowPresentation.Item) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 7) {
            Image(systemName: TaskWorkflowPresentation.icon(for: item))
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(TaskWorkflowPresentation.isFailure(item) ? DT.redText : DT.blueText)
                .frame(width: 12)
            VStack(alignment: .leading, spacing: 3) {
                Text(TaskWorkflowPresentation.title(for: item, locale: locale))
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                if let tool = item.toolName {
                    Text(localizedFormat("来源工具：%@", locale: locale, tool))
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(1)
                }
            }
            .help(TaskWorkflowPresentation.title(for: item, locale: locale))
            Spacer(minLength: 4)
            if let duration = TaskWorkflowPresentation.durationText(
                for: item,
                locale: locale
            ) {
                Text(duration)
                    .font(.system(size: 9.5, design: .monospaced))
                    .foregroundStyle(DT.textWeak)
            }
            Text(TaskWorkflowPresentation.time(
                for: item.event,
                locale: locale
            ))
                .font(.system(size: 9.5, design: .monospaced))
                .foregroundStyle(DT.textFaint)
        }
    }
}

private struct TaskWorkflowFreshnessLabel: View {
    let lastEventAt: Date
    let running: Bool
    let language: AppLanguage
    let locale: Locale

    var body: some View {
        TimelineView(.periodic(from: .now, by: running ? 5 : 30)) { timeline in
            let age = max(0, timeline.date.timeIntervalSince(lastEventAt))
            Text(TaskWorkflowFreshnessPresentation.text(
                age: age,
                running: running,
                language: language,
                locale: locale
            ))
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(
                    TaskWorkflowFreshnessPresentation.isQuiet(
                        age: age,
                        running: running
                    ) ? DT.amberText : DT.textFaint
                )
                .lineLimit(1)
        }
    }
}

enum TaskWorkflowFreshnessPresentation {
    static let quietThreshold: TimeInterval = 120

    static func isQuiet(age: TimeInterval, running: Bool) -> Bool {
        running && age >= quietThreshold
    }

    static func text(
        age: TimeInterval,
        running: Bool,
        language: AppLanguage,
        locale: Locale
    ) -> String {
        if isQuiet(age: age, running: running) {
            return localizedFormat(
                "%@ 无新事件",
                locale: locale,
                ZhFormat.waitDuration(age, language: language)
            )
        }
        if age < 5 {
            return localized("实时", locale: locale)
        }
        return localizedFormat(
            "%@更新",
            locale: locale,
            ZhFormat.relativeAgo(age, language: language)
        )
    }
}

/// Only the visible relative-time label owns a clock. Task-card facts stay
/// stable while elapsed text advances, including when ActRealm is visible but
/// another app has focus.

private struct TaskRelativeStatusLabel: View {
    let task: LaneTask
    let language: AppLanguage
    let locale: Locale

    var body: some View {
        TimelineView(.periodic(from: .now, by: refreshInterval)) { timeline in
            Text(statusText(now: timeline.date))
        }
    }

    private var refreshInterval: TimeInterval {
        switch task.status {
        case .waiting: 1
        case .running: 5
        case .failed, .done, .idle: 30
        }
    }

    private func statusText(now: Date) -> String {
        switch task.status {
        case .waiting:
            let since = task.oldestOpenOutboxAt ?? task.activitySince ?? task.lastEventAt
            return localizedFormat(
                "已等 %@",
                locale: locale,
                ZhFormat.waitDuration(now.timeIntervalSince(since), language: language)
            )
        case .running:
            return turnTiming(now: now)
        case .failed:
            return localizedFormat(
                "运行失败 · %@",
                locale: locale,
                ZhFormat.relativeAgo(now.timeIntervalSince(task.lastEventAt), language: language)
            )
        case .done:
            return localizedFormat(
                "本轮已完成 · %@",
                locale: locale,
                ZhFormat.relativeAgo(now.timeIntervalSince(task.lastEventAt), language: language)
            )
        case .idle:
            return localizedFormat(
                "最近活动 · %@",
                locale: locale,
                ZhFormat.relativeAgo(now.timeIntervalSince(task.lastEventAt), language: language)
            )
        }
    }

    private func turnTiming(now: Date) -> String {
        let started = task.turnStartedAt ?? task.activitySince ?? task.lastEventAt
        let ended = task.turnEndedAt ?? now
        let total = ZhFormat.waitDuration(
            max(0, ended.timeIntervalSince(started)),
            language: language
        )
        if task.turnEndedAt == nil,
           let phase = task.activitySince,
           phase > started {
            return localizedFormat(
                "本轮 %@ · 当前阶段 %@",
                locale: locale,
                total,
                ZhFormat.waitDuration(max(0, now.timeIntervalSince(phase)), language: language)
            )
        }
        return localizedFormat("本轮 %@", locale: locale, total)
    }
}

private struct CheckpointSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.locale) private var locale
    let client: RuntimeClient
    let sessionID: String
    let taskTitle: String
    let language: AppLanguage
    @State private var checkpoints: [RuntimeTaskCheckpoint] = []
    @State private var selectedID: String?
    @State private var label = ""
    @State private var preflight: RuntimeCheckpointPreflight?
    @State private var requestedAction: String?
    @State private var loading = false
    @State private var error: String?
    @State private var statusMessage: String?
    @State private var confirmingGitSnapshot = false
    @State private var pendingDelete: RuntimeTaskCheckpoint?

    private var selected: RuntimeTaskCheckpoint? {
        checkpoints.first { $0.id == selectedID } ?? checkpoints.first
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "bookmark.square.fill")
                    .foregroundStyle(DT.blueText)
                VStack(alignment: .leading, spacing: 2) {
                    Text(localized("Checkpoint 与恢复", locale: locale))
                        .font(.system(size: 14, weight: .bold))
                    Text(taskTitle)
                        .font(.system(size: 9.5))
                        .foregroundStyle(DT.textWeak)
                        .lineLimit(1)
                }
                Spacer()
                if loading { ProgressView().controlSize(.small) }
                Button(localized("完成", locale: locale)) { dismiss() }
            }
            .padding(.horizontal, 18)
            .padding(.vertical, 14)
            .background(DT.cardMedium)
            .overlay(alignment: .bottom) {
                Rectangle().fill(DT.separator).frame(height: 1)
            }

            HStack(alignment: .top, spacing: 0) {
                VStack(alignment: .leading, spacing: 10) {
                    TextField(
                        localized("可选标签", locale: locale),
                        text: $label
                    )
                    .textFieldStyle(.roundedBorder)
                    HStack(spacing: 7) {
                        Button {
                            Task { await create(kind: "metadata") }
                        } label: {
                            Label(
                                localized("保存元数据", locale: locale),
                                systemImage: "bookmark"
                            )
                        }
                        .buttonStyle(ActionButtonStyle(kind: .primary, compact: true))
                        Button {
                            confirmingGitSnapshot = true
                        } label: {
                            Label(
                                localized("创建 Git 快照", locale: locale),
                                systemImage: "point.3.connected.trianglepath.dotted"
                            )
                        }
                        .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                    }
                    Text(localized(
                        "元数据 Checkpoint 不会提交代码；Git 快照不改变当前工作区，也不包含未跟踪文件。",
                        locale: locale
                    ))
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.textFaint)
                    .fixedSize(horizontal: false, vertical: true)

                    Divider()
                    if checkpoints.isEmpty && !loading {
                        VStack(spacing: 7) {
                            Image(systemName: "bookmark")
                                .font(.system(size: 23))
                                .foregroundStyle(DT.textFaint)
                            Text(localized("尚未创建 Checkpoint", locale: locale))
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(DT.textWeak)
                        }
                        .frame(maxWidth: .infinity, minHeight: 160)
                    } else {
                        ScrollView {
                            LazyVStack(spacing: 7) {
                                ForEach(checkpoints) { checkpoint in
                                    Button {
                                        selectedID = checkpoint.id
                                        preflight = nil
                                        requestedAction = nil
                                    } label: {
                                        checkpointRow(checkpoint)
                                    }
                                    .buttonStyle(.plain)
                                }
                            }
                        }
                    }
                }
                .padding(16)
                .frame(width: 330)

                Rectangle().fill(DT.separator).frame(width: 1)

                ScrollView {
                    VStack(alignment: .leading, spacing: 12) {
                        if let selected {
                            checkpointDetail(selected)
                        } else {
                            Text(localized(
                                "选择一个 Checkpoint 查看恢复预检",
                                locale: locale
                            ))
                            .foregroundStyle(DT.textFaint)
                            .frame(maxWidth: .infinity, minHeight: 260)
                        }
                        if let error {
                            Text(error)
                                .font(.system(size: 9.5))
                                .foregroundStyle(DT.redText)
                        }
                        if let statusMessage {
                            Text(statusMessage)
                                .font(.system(size: 9.5))
                                .foregroundStyle(DT.greenText)
                        }
                    }
                    .padding(18)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 780, idealWidth: 880, minHeight: 520, idealHeight: 600)
        .background(DT.cardStrong)
        .task { await load() }
        .confirmationDialog(
            localized("创建 Git 快照？", locale: locale),
            isPresented: $confirmingGitSnapshot,
            titleVisibility: .visible
        ) {
            Button(localized("创建 Git 快照", locale: locale)) {
                Task { await create(kind: "git_snapshot") }
            }
            Button(localized("取消", locale: locale), role: .cancel) {}
        } message: {
            Text(localized(
                "仅保存已跟踪改动的 stash-like Git 对象和 ActRealm 元数据；不会 commit、切换分支或修改工作区。",
                locale: locale
            ))
        }
        .alert(
            localized("删除 Checkpoint？", locale: locale),
            isPresented: Binding(
                get: { pendingDelete != nil },
                set: { if !$0 { pendingDelete = nil } }
            )
        ) {
            Button(localized("删除", locale: locale), role: .destructive) {
                guard let checkpoint = pendingDelete else { return }
                pendingDelete = nil
                Task { await delete(checkpoint) }
            }
            Button(localized("取消", locale: locale), role: .cancel) {}
        } message: {
            Text(localized(
                "只删除 ActRealm Checkpoint 元数据和自有 Git 引用；不会删除 Provider 会话或工作区文件。",
                locale: locale
            ))
        }
    }

    private func checkpointRow(_ checkpoint: RuntimeTaskCheckpoint) -> some View {
        HStack(spacing: 9) {
            Image(systemName: checkpoint.kind == "git_snapshot"
                ? "point.3.connected.trianglepath.dotted"
                : "bookmark")
                .foregroundStyle(checkpoint.kind == "git_snapshot" ? DT.blueText : DT.textWeak)
                .frame(width: 18)
            VStack(alignment: .leading, spacing: 2) {
                Text(checkpoint.label ?? checkpointKindText(checkpoint.kind))
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
                Text(checkpointDate(checkpoint.createdAt))
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.textFaint)
            }
            Spacer()
            if checkpoint.id == selected?.id {
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(DT.blueText)
            }
        }
        .padding(9)
        .background(
            checkpoint.id == selected?.id ? DT.blueBg : DT.neutralChipBg,
            in: RoundedRectangle(cornerRadius: 9)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 9)
                .strokeBorder(DT.neutralChipStroke, lineWidth: 1)
        )
    }

    @ViewBuilder
    private func checkpointDetail(_ checkpoint: RuntimeTaskCheckpoint) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text(checkpoint.label ?? checkpointKindText(checkpoint.kind))
                        .font(.system(size: 14, weight: .bold))
                    Text("\(checkpoint.provider) · \(checkpointDate(checkpoint.createdAt))")
                        .font(.system(size: 9))
                        .foregroundStyle(DT.textWeak)
                }
                Spacer()
                Chip(
                    text: checkpoint.kind == "git_snapshot"
                        ? localized("Git 快照", locale: locale)
                        : localized("仅元数据", locale: locale),
                    tone: checkpoint.kind == "git_snapshot" ? .blue : .neutral,
                    fontSize: 8.5
                )
            }
            checkpointRepositoryFacts(checkpoint)
            if !checkpoint.validations.isEmpty {
                VStack(alignment: .leading, spacing: 5) {
                    Text(localized("当时的验证证据", locale: locale))
                        .font(.system(size: 9.5, weight: .semibold))
                    ForEach(checkpoint.validations.suffix(4)) { validation in
                        Text("\(validation.kind) · \(validation.state)")
                            .font(.system(size: 9))
                            .foregroundStyle(DT.textWeak)
                    }
                    Text(localized(
                        "恢复后这些结果只作为历史证据；新 Turn 必须重新验证。",
                        locale: locale
                    ))
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.amberText)
                }
            }
            if !checkpoint.limitations.isEmpty {
                Text(checkpoint.limitations.map(checkpointReasonText).joined(separator: " · "))
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.amberText)
            }
            HStack(spacing: 7) {
                Button(localized("恢复会话", locale: locale)) {
                    Task { await prepare(action: "resume_session") }
                }
                .disabled(checkpoint.providerResumeCapability == "unsupported")
                if checkpoint.repository.gitSnapshot {
                    Button(localized("恢复代码", locale: locale)) {
                        Task { await prepare(action: "restore_code") }
                    }
                    Button(localized("回退代码", locale: locale)) {
                        Task { await prepare(action: "rollback_code") }
                    }
                }
                Spacer()
                Button(role: .destructive) {
                    pendingDelete = checkpoint
                } label: {
                    Label(localized("删除", locale: locale), systemImage: "trash")
                }
            }
            .buttonStyle(.bordered)
            if let preflight, preflight.checkpointId == checkpoint.id {
                checkpointPreflightCard(preflight)
            }
        }
    }

    private func checkpointRepositoryFacts(_ checkpoint: RuntimeTaskCheckpoint) -> some View {
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 6) {
            DetailLine(
                label: localized("分支", locale: locale),
                value: checkpoint.repository.branch ?? "—",
                emphasized: false
            )
            DetailLine(
                label: "HEAD",
                value: checkpoint.repository.head ?? "—",
                emphasized: false
            )
            DetailLine(
                label: localized("工作区", locale: locale),
                value: checkpoint.repository.worktreeKind ?? localized("不可用", locale: locale),
                emphasized: false
            )
            DetailLine(
                label: localized("改动", locale: locale),
                value: localizedFormat(
                    "%lld 个文件",
                    locale: locale,
                    Int64(checkpoint.repository.changedFiles ?? 0)
                ),
                emphasized: checkpoint.repository.dirty == true
            )
        }
    }

    private func checkpointPreflightCard(_ preflight: RuntimeCheckpointPreflight) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack {
                Image(systemName: preflight.allowed
                    ? "checkmark.shield.fill"
                    : "exclamationmark.shield.fill")
                    .foregroundStyle(preflight.allowed ? DT.greenText : DT.redText)
                Text(preflight.allowed
                    ? localized("预检通过", locale: locale)
                    : localized("预检阻止执行", locale: locale))
                    .font(.system(size: 10, weight: .semibold))
                Spacer()
                Text(checkpointActionText(preflight.action))
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.textWeak)
            }
            ForEach(preflight.blockers, id: \.self) { blocker in
                Label(checkpointReasonText(blocker), systemImage: "xmark.circle")
                    .font(.system(size: 9))
                    .foregroundStyle(DT.redText)
            }
            ForEach(preflight.warnings, id: \.self) { warning in
                Label(checkpointReasonText(warning), systemImage: "info.circle")
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.amberText)
            }
            if preflight.allowed, let requestedAction {
                Button(checkpointExecuteText(requestedAction)) {
                    Task { await execute(action: requestedAction) }
                }
                .buttonStyle(ActionButtonStyle(kind: .primary, compact: true))
            }
        }
        .padding(10)
        .background(
            preflight.allowed ? DT.greenBg : DT.redBg,
            in: RoundedRectangle(cornerRadius: 10)
        )
        .textSelection(.enabled)
    }

    @MainActor
    private func load() async {
        loading = true
        defer { loading = false }
        do {
            checkpoints = try await client.sessionCheckpoints(sessionId: sessionID)
            if selectedID == nil || !checkpoints.contains(where: { $0.id == selectedID }) {
                selectedID = checkpoints.first?.id
            }
            error = nil
        } catch {
            self.error = localized("Checkpoint 暂时无法读取", locale: locale)
        }
    }

    @MainActor
    private func create(kind: String) async {
        loading = true
        defer { loading = false }
        do {
            let checkpoint = try await client.createCheckpoint(
                sessionId: sessionID,
                kind: kind,
                label: label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    ? nil : label
            )
            label = ""
            statusMessage = localized("Checkpoint 已创建", locale: locale)
            error = nil
            checkpoints = try await client.sessionCheckpoints(sessionId: sessionID)
            selectedID = checkpoint.id
        } catch {
            self.error = localized(
                kind == "git_snapshot"
                    ? "Git 快照创建失败；可能没有已跟踪改动"
                    : "Checkpoint 创建失败",
                locale: locale
            )
        }
    }

    @MainActor
    private func prepare(action: String) async {
        guard let selected else { return }
        loading = true
        defer { loading = false }
        do {
            preflight = try await client.checkpointPreflight(
                checkpointId: selected.id,
                action: action
            )
            requestedAction = action
            error = nil
        } catch {
            self.error = localized("恢复预检失败", locale: locale)
        }
    }

    @MainActor
    private func execute(action: String) async {
        guard let selected, preflight?.allowed == true else { return }
        loading = true
        defer { loading = false }
        if action == "resume_session" {
            let (_, jumpError) = await client.jumpSession(sessionID)
            if jumpError == nil {
                statusMessage = localized("Checkpoint 操作已完成", locale: locale)
                error = nil
                preflight = nil
                requestedAction = nil
            } else {
                self.error = localized("Checkpoint 操作失败；工作区保持原状", locale: locale)
            }
            return
        }
        do {
            _ = try await client.applyCheckpointAction(
                checkpointId: selected.id,
                action: action
            )
            statusMessage = localized("Checkpoint 操作已完成", locale: locale)
            error = nil
            preflight = nil
            requestedAction = nil
        } catch {
            self.error = localized("Checkpoint 操作失败；工作区保持原状", locale: locale)
        }
    }

    @MainActor
    private func delete(_ checkpoint: RuntimeTaskCheckpoint) async {
        loading = true
        defer { loading = false }
        do {
            try await client.deleteCheckpoint(checkpointId: checkpoint.id)
            checkpoints.removeAll { $0.id == checkpoint.id }
            selectedID = checkpoints.first?.id
            preflight = nil
            requestedAction = nil
            statusMessage = localized("Checkpoint 已删除", locale: locale)
            error = nil
        } catch {
            self.error = localized("Checkpoint 删除失败", locale: locale)
        }
    }

    private func checkpointDate(_ milliseconds: UInt64) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        return formatter.string(from: ZhFormat.date(fromMillis: milliseconds))
    }

    private func checkpointKindText(_ kind: String) -> String {
        kind == "git_snapshot"
            ? localized("Git 快照", locale: locale)
            : localized("元数据 Checkpoint", locale: locale)
    }

    private func checkpointActionText(_ action: String) -> String {
        switch action {
        case "resume_session": localized("恢复会话", locale: locale)
        case "restore_code": localized("恢复代码", locale: locale)
        case "rollback_code": localized("回退代码", locale: locale)
        default: action
        }
    }

    private func checkpointExecuteText(_ action: String) -> String {
        switch action {
        case "resume_session": localized("打开原会话", locale: locale)
        case "restore_code": localized("执行代码恢复", locale: locale)
        case "rollback_code": localized("执行代码回退", locale: locale)
        default: localized("执行", locale: locale)
        }
    }

    private func checkpointReasonText(_ reason: String) -> String {
        switch reason {
        case "provider_resume_unsupported": localized("Provider 不支持准确恢复会话", locale: locale)
        case "provider_session_unavailable": localized("Provider 原会话已经不可用", locale: locale)
        case "code_state_not_changed": localized("恢复会话不会修改代码", locale: locale)
        case "repository_unavailable": localized("仓库或 Worktree 不可用", locale: locale)
        case "repository_identity_changed": localized("仓库身份已变化", locale: locale)
        case "branch_changed": localized("当前分支与 Checkpoint 不同", locale: locale)
        case "head_changed": localized("当前 HEAD 与 Checkpoint 不同", locale: locale)
        case "working_tree_dirty": localized("当前存在未提交改动，拒绝覆盖", locale: locale)
        case "untracked_changes_present": localized("存在未跟踪文件，拒绝回退", locale: locale)
        case "git_snapshot_unavailable": localized("该 Checkpoint 没有 Git 快照", locale: locale)
        case "git_snapshot_changed": localized("Git 快照对象与记录不一致", locale: locale)
        case "working_tree_not_checkpoint": localized("当前改动不等于 Checkpoint，拒绝回退", locale: locale)
        case "patch_conflict": localized("补丁预检存在冲突", locale: locale)
        case "untracked_not_captured": localized("未跟踪文件未包含在 Git 快照中", locale: locale)
        case "validation_is_historical": localized("验证结果是历史证据，恢复后需重新验证", locale: locale)
        default: reason
        }
    }
}

private struct ReviewDiffSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.locale) private var locale
    @ObservedObject var client: RuntimeClient
    let sessionID: String
    let language: AppLanguage
    @State private var response: RuntimeReviewDiffResponse?
    @State private var selectedPath: String?
    @State private var loading = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(localized("本机 Diff", locale: locale))
                        .font(.system(size: 16, weight: .bold))
                    Text(localized(
                        "只在本机按需读取；不会发送到其他本机应用",
                        locale: locale
                    ))
                        .font(.system(size: 10))
                        .foregroundStyle(DT.textWeak)
                }
                Spacer()
                if loading { ProgressView().controlSize(.small) }
                Button(localized("关闭", locale: locale)) { dismiss() }
            }

            if let error {
                Text(error).foregroundStyle(DT.redText)
            } else if let response {
                HStack(spacing: 8) {
                    Text(response.base.map { "Base · \($0)" } ?? "Base · —")
                    Text(localized(reviewDiffAttribution(response.attribution), locale: locale))
                }
                .font(.system(size: 10))
                .foregroundStyle(DT.textWeak)

                HStack(alignment: .top, spacing: 10) {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 4) {
                            if response.files.isEmpty {
                                Text(localized("没有可显示的文件差异", locale: locale))
                                    .font(.system(size: 10))
                                    .foregroundStyle(DT.textFaint)
                            }
                            ForEach(response.files) { file in
                                Button {
                                    selectedPath = file.path
                                    Task { await load(path: file.path) }
                                } label: {
                                    HStack(spacing: 6) {
                                        Image(systemName: file.state == "untracked"
                                            ? "questionmark.square.dashed"
                                            : "doc.text")
                                        Text(file.path)
                                            .lineLimit(2)
                                            .multilineTextAlignment(.leading)
                                        Spacer(minLength: 2)
                                    }
                                    .padding(6)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(
                                        selectedPath == file.path
                                            ? DT.blueBg : Color.clear,
                                        in: RoundedRectangle(cornerRadius: 7)
                                    )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    .frame(width: 230, height: 360)

                    ScrollView([.horizontal, .vertical]) {
                        if let patch = response.selected {
                            Text(patch.patch.isEmpty
                                ? localized("该文件没有文本 Patch", locale: locale)
                                : patch.patch)
                                .font(.system(size: 10, design: .monospaced))
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .topLeading)
                            if patch.truncated {
                                Text(localized("Patch 已截断到 256 KiB", locale: locale))
                                    .font(.system(size: 9))
                                    .foregroundStyle(DT.amberText)
                            }
                        } else {
                            Text(reviewDiffEmptyText(response.limitation))
                                .font(.system(size: 10))
                                .foregroundStyle(DT.textFaint)
                        }
                    }
                    .padding(8)
                    .frame(maxWidth: .infinity, minHeight: 360, maxHeight: 360)
                    .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 9))
                }
            }
        }
        .padding(16)
        .frame(minWidth: 780, minHeight: 470)
        .task { await load(path: nil) }
    }

    @MainActor
    private func load(path: String?) async {
        loading = true
        error = nil
        defer { loading = false }
        do {
            response = try await client.sessionReviewDiff(sessionId: sessionID, path: path)
        } catch {
            self.error = localized("Diff 暂时无法读取", locale: locale)
        }
    }

    private func reviewDiffAttribution(_ attribution: String) -> String {
        switch attribution {
        case "exact": "精确归因"
        case "bounded_window": "有界归因"
        case "concurrent_changes": "存在并发改动"
        default: "未归因"
        }
    }

    private func reviewDiffEmptyText(_ limitation: String?) -> String {
        switch limitation {
        case "untracked_patch_not_read":
            localized("未跟踪文件只列出名称，不自动读取内容", locale: locale)
        case "diff_file_limit":
            localized("文件列表已截断", locale: locale)
        default:
            localized("选择左侧文件查看本机 Patch", locale: locale)
        }
    }
}

private struct DetailLine: View {
    @Environment(\.locale) private var locale
    let label: String
    let value: String
    var emphasized = false

    var body: some View {
        HStack(spacing: 10) {
            Text(localized(label, locale: locale)).foregroundStyle(DT.textWeak)
            Spacer(minLength: 4)
            Text(localized(value, locale: locale))
                .fontWeight(emphasized ? .semibold : .regular)
                .foregroundStyle(emphasized ? DT.amberText : DT.textPrimary)
                .lineLimit(2)
                .fixedSize(horizontal: false, vertical: true)
                .help(localized(value, locale: locale))
        }
        .frame(maxWidth: .infinity)
    }
}

private struct ProgressTrack: View {
    let fraction: Double
    var color: Color = DT.blue

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(DT.progressTrack)
                Capsule().fill(color)
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
                Spacer()
                Button {
                    Task { await model.refreshQuotaNow() }
                } label: {
                    Image(systemName: "arrow.clockwise")
                        .font(.system(size: 11, weight: .semibold))
                }
                .buttonStyle(.plain)
                .disabled(model.isQuotaRefreshBusy || !model.canControlRuntime)
                .help(Text(model.quotaRefreshMessage ?? AppLocalization.localized(
                    "更新 Claude 额度", language: model.appLanguage
                )))
                .accessibilityLabel(Text("更新 Claude 额度"))
            }
            if let message = model.quotaRefreshMessage {
                Text(message)
                    .font(.system(size: 9))
                    .foregroundStyle(DT.textWeak)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, 6)
            }

            if model.uiSettings.tokenUsageDisplayMode != .hidden {
                TokenUsageSummaryCard(
                    totals: model.tokenUsage,
                    mode: model.uiSettings.tokenUsageDisplayMode
                )
                .padding(.top, 11)
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

            Spacer(minLength: 8)
            RuntimeLiveStatus()
                .padding(.top, 10)
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
    }

    private var quotaList: some View {
        LazyVStack(spacing: 8) {
            ForEach(model.derived.quotaSlots) { slot in
                QuotaCard(slot: slot)
                    .frame(maxWidth: .infinity)
            }
        }
        .padding(.top, 11)
        .frame(maxWidth: .infinity)
    }
}

private struct TokenUsageSummaryCard: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @Environment(\.openWindow) private var openWindow
    let totals: TokenUsageTotals
    let mode: TokenUsageDisplayMode

    private func formattedTokens(_ value: UInt64) -> String {
        TokenDashboardPresentation.tokenText(value, unitStyle: model.uiSettings.tokenUsageUnitStyle, locale: locale)
    }

    var body: some View {
        Button {
            openWindow(id: "token-dashboard")
        } label: {
            if !TokenDashboardPresentation.hasObservedData(totals) {
                provisionalCard
            } else if mode == .compact {
                compactCard
            } else {
                standardCard
            }
        }
        .buttonStyle(.plain)
        .help(localized("打开 Token 仪表板", locale: locale))
    }

    private var provisionalCard: some View {
        HStack(spacing: 9) {
            Image(systemName: totals.dataQuality == "suspect"
                ? "exclamationmark.triangle.fill"
                : "arrow.triangle.2.circlepath")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(DT.amberText)
                .frame(width: 28, height: 28)
                .background(DT.amberBg.opacity(0.72), in: RoundedRectangle(cornerRadius: 8))
            VStack(alignment: .leading, spacing: 2) {
                Text("TOKEN DATA")
                    .font(.system(size: 9, weight: .heavy))
                    .kerning(0.45)
                    .foregroundStyle(DT.textSecondary)
                Text(collectionStatusText)
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(DT.amberText)
                    .lineLimit(1)
                Text(localized("打开仪表板查看采集状态与已观测数据", locale: locale))
                    .font(.system(size: 8))
                    .foregroundStyle(DT.textFaint)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            Image(systemName: "chevron.right")
                .font(.system(size: 8, weight: .bold))
                .foregroundStyle(DT.textFaint)
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(DT.cardStrong.opacity(0.86), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(DT.amberStroke, lineWidth: 1))
    }

    private var standardCard: some View {
        VStack(alignment: .leading, spacing: 11) {
            HStack(spacing: 8) {
                VStack(alignment: .leading, spacing: 1) {
                    Text("TOKEN USAGE")
                        .font(.system(size: 9.5, weight: .heavy))
                        .kerning(0.55)
                        .foregroundStyle(DT.textPrimary)
                    Text(localized(hasTrustworthyTotals ? "本机 Agent 累计" : "本机已观测用量", locale: locale))
                        .font(.system(size: 8.5, weight: .medium))
                        .foregroundStyle(DT.textFaint)
                }
                Spacer(minLength: 4)
                collectionIndicator
                Image(systemName: "chevron.right")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(DT.textFaint)
            }

            HStack(alignment: .bottom, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(localized("今日", locale: locale))
                        .font(.system(size: 8.5, weight: .semibold))
                        .foregroundStyle(DT.textWeak)
                    Text(formattedTokens(totals.today))
                        .font(.system(size: 22, weight: .bold, design: .rounded))
                        .foregroundStyle(DT.textPrimary)
                        .monospacedDigit()
                        .lineLimit(1)
                        .minimumScaleFactor(0.72)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                VStack(alignment: .leading, spacing: 7) {
                    compactMetric(localized("本月", locale: locale), totals.month)
                    compactMetric(localized("累计", locale: locale), totals.total)
                }
                .frame(width: 104, alignment: .leading)
            }

            providerChips

            HStack(spacing: 5) {
                Text(recordingNote)
                    .font(.system(size: 8.3))
                    .foregroundStyle(DT.textFaint)
                    .lineLimit(1)
                Spacer(minLength: 4)
                Text(collectionStatusText)
                    .font(.system(size: 8.3, weight: .semibold))
                    .foregroundStyle(collectionStatusColor)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            LinearGradient(
                colors: [DT.cardStrong.opacity(0.92), DT.blueBg.opacity(0.42)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            ),
            in: RoundedRectangle(cornerRadius: 14, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(DT.hairlineSoft, lineWidth: 1)
        )
        .shadow(color: DT.softShadow, radius: 3, y: 1)
    }

    private var compactCard: some View {
        HStack(spacing: 9) {
            Image(systemName: "chart.bar.xaxis")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(DT.blueText)
                .frame(width: 28, height: 28)
                .background(DT.blueBg.opacity(0.66), in: RoundedRectangle(cornerRadius: 8))
            VStack(alignment: .leading, spacing: 2) {
                Text("TOKEN USAGE")
                    .font(.system(size: 9, weight: .heavy))
                    .kerning(0.45)
                    .foregroundStyle(DT.textSecondary)
                Text(recordingNote)
                    .font(.system(size: 8))
                    .foregroundStyle(DT.textFaint)
                    .lineLimit(1)
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 1) {
                Text(localized("今日", locale: locale))
                    .font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(DT.textWeak)
                Text(formattedTokens(totals.today))
                    .font(.system(size: 15, weight: .bold, design: .rounded))
                    .foregroundStyle(DT.textPrimary)
                    .monospacedDigit()
            }
            Image(systemName: "chevron.right")
                .font(.system(size: 8, weight: .bold))
                .foregroundStyle(DT.textFaint)
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(DT.cardStrong.opacity(0.86), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(DT.hairlineSoft, lineWidth: 1))
    }

    private func compactMetric(_ label: String, _ value: UInt64) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 5) {
            Text(label)
                .font(.system(size: 8, weight: .semibold))
                .foregroundStyle(DT.textWeak)
                .frame(width: 24, alignment: .leading)
            Text(formattedTokens(value))
                .font(.system(size: 10.5, weight: .semibold, design: .rounded))
                .foregroundStyle(DT.textSecondary)
                .monospacedDigit()
                .lineLimit(1)
                .minimumScaleFactor(0.75)
        }
    }

    @ViewBuilder
    private var collectionIndicator: some View {
        if totals.collectionInProgress {
            ProgressView()
                .controlSize(.mini)
        } else {
            Circle()
                .fill(collectionStatusColor)
                .frame(width: 6, height: 6)
        }
    }

    private var providerChips: some View {
        HStack(spacing: 6) {
            ForEach(knownProviderTotals.prefix(2)) { provider in
                HStack(spacing: 4) {
                    Circle()
                        .fill(providerColor(provider.provider))
                        .frame(width: 5, height: 5)
                    Text(providerName(provider.provider))
                        .font(.system(size: 8.2, weight: .semibold))
                        .foregroundStyle(DT.textWeak)
                    Text(provider.total > 0
                        ? formattedTokens(provider.total)
                        : localized("待采集", locale: locale))
                        .font(.system(size: 8.2, weight: .semibold, design: .rounded))
                        .foregroundStyle(provider.total > 0 ? DT.textSecondary : DT.textFaint)
                        .monospacedDigit()
                }
                .padding(.horizontal, 7)
                .padding(.vertical, 4)
                .background(DT.cardMedium.opacity(0.72), in: Capsule())
                .overlay(Capsule().strokeBorder(DT.neutralBadgeStroke, lineWidth: 1))
            }
            Spacer(minLength: 0)
        }
    }

    private var recordingNote: String {
        guard let recordedFrom = totals.recordedFrom else {
            return localized("等待本机 Agent 用量", locale: locale)
        }
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.setLocalizedDateFormatFromTemplate("MMM d")
        let date = formatter.string(from: ZhFormat.date(fromMillis: recordedFrom))
        return localizedFormat(
            "从 %@ 开始记录 · 按本机日历统计",
            locale: locale,
            date
        )
    }

    private var collectionStatusText: String {
        if totals.dataQuality == "suspect" {
            return localizedFormat(
                "发现 %lld 项用量异常 · 打开仪表板查看",
                locale: locale,
                Int64(totals.suspectCount)
            )
        }
        switch totals.collectionState {
        case "scanning":
            return localized("首次扫描中，统计仍会增长", locale: locale)
        case "partial":
            return localized("历史数据部分可用", locale: locale)
        case "unavailable":
            return localized("暂时无法读取 Agent 用量", locale: locale)
        case "ready":
            if totals.collectionInProgress {
                return localized("正在刷新本机用量", locale: locale)
            }
            guard let timestamp = totals.lastSuccessfulAt else {
                return localized("本机用量已就绪", locale: locale)
            }
            return localizedFormat(
                "已就绪 · %@ 更新",
                locale: locale,
                ZhFormat.syncClock(ZhFormat.date(fromMillis: timestamp))
            )
        default:
            return localized("等待首次用量扫描", locale: locale)
        }
    }

    private var collectionStatusColor: Color {
        if totals.dataQuality == "suspect" {
            return DT.amberText
        }
        switch totals.collectionState {
        case "partial", "unavailable": return DT.amberText
        case "ready": return DT.greenText
        default: return DT.blueText
        }
    }

    private var hasTrustworthyTotals: Bool {
        totals.collectionState == "ready" && totals.dataQuality != "suspect"
    }

    private var knownProviderTotals: [TokenUsageProviderTotal] {
        var values = totals.byProvider
        for provider in ["codex", "claude"] where !values.contains(where: {
            $0.provider.caseInsensitiveCompare(provider) == .orderedSame
        }) {
            values.append(TokenUsageProviderTotal(provider: provider, total: 0))
        }
        return values.sorted { left, right in
            if (left.total > 0) != (right.total > 0) {
                return left.total > 0
            }
            if left.total != right.total {
                return left.total > right.total
            }
            let order = ["codex": 0, "claude": 1]
            return (order[left.provider.lowercased()] ?? 2)
                < (order[right.provider.lowercased()] ?? 2)
        }
    }

    private func providerColor(_ provider: String) -> Color {
        provider.lowercased() == "claude" ? DT.amberText : DT.blueText
    }

    private func providerName(_ provider: String) -> String {
        switch provider.lowercased() {
        case "codex": "Codex"
        case "claude": "Claude"
        default: provider
        }
    }

}

private struct RuntimeLiveStatus: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Rectangle()
                .fill(DT.separator)
                .frame(height: 1)

            HStack(spacing: 7) {
                StatusDot(color: color, size: 6, glow: model.bridgeStatus.isListening)
                Text(status)
                    .font(.system(size: 10.5, weight: .semibold))
            }

            Text(localizedFormat(
                "最近同步 · %@",
                locale: locale,
                model.lastSyncAt.map(ZhFormat.syncClock) ?? "—"
            ))
                .font(.system(size: 9.5))
                .padding(.leading, 1)
        }
        .foregroundStyle(DT.textWeak)
        .frame(maxWidth: .infinity, alignment: .leading)
        .allowsHitTesting(false)
    }

    private var status: String {
        let key = switch model.bridgeStatus {
        case .listening: "Runtime · 本机在线"
        case .starting: "Runtime · 本机启动中"
        case .absent: "Runtime · 本机未连接"
        }
        return localized(key, locale: locale)
    }

    private var color: Color {
        switch model.bridgeStatus {
        case .listening: DT.greenDot
        case .starting: DT.amberDot
        case .absent: DT.redText
        }
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
    @Environment(\.locale) private var locale
    let slot: QuotaSlot

    @ViewBuilder
    var body: some View {
        switch model.uiSettings.quotaDisplayMode {
        case .full:
            standardCard
        case .compact:
            compactCard
        case .singleLine:
            singleLineCard
        }
    }

    private var standardCard: some View {
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

    private var compactCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 7) {
                ProviderAvatar(kind: slot.slot.provider, size: 20)
                Text(title)
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.78)
                    .layoutPriority(1)
                Spacer(minLength: 4)
                statusChip
                    .fixedSize(horizontal: true, vertical: false)
            }

            compactContent
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, minHeight: 84, alignment: .leading)
        .background(cardFill, in: RoundedRectangle(cornerRadius: 13))
        .overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(DT.hairlineSoft, lineWidth: 1))
        .shadow(color: DT.softShadow, radius: 2, y: 1)
        .help(compactHelp)
    }

    private var singleLineCard: some View {
        HStack(spacing: 5) {
            ProviderAvatar(kind: slot.slot.provider, size: 18)

            Text(title)
                .font(.system(size: 11, weight: .bold))
                .foregroundStyle(DT.textPrimary)
                .lineLimit(1)
                .minimumScaleFactor(0.85)
                .layoutPriority(2)

            if let remaining = compactRemaining {
                ProgressTrack(fraction: remaining / 100, color: compactTone)
                    .frame(minWidth: 34, idealWidth: 64, maxWidth: .infinity)
                    .frame(height: 5)
                    .layoutPriority(1)

                Text("\(Int(remaining.rounded()))%")
                    .font(.system(size: 13.5, weight: .heavy))
                    .foregroundStyle(remaining < 50 ? DT.amberText : DT.textPrimary)
                    .monospacedDigit()
                    .fixedSize(horizontal: true, vertical: false)
                    .frame(minWidth: 34, alignment: .trailing)
            } else {
                Spacer(minLength: 4)
                Text(compactStatus)
                    .font(.system(size: 9.5, weight: .semibold))
                    .foregroundStyle(compactTone)
                    .lineLimit(1)
            }

            ZStack {
                Circle()
                    .fill(compactTone.opacity(0.14))
                    .frame(width: 15, height: 15)
                StatusDot(color: compactTone, size: 7, glow: compactIsAvailable)
            }
        }
        .padding(.horizontal, 8)
        .frame(maxWidth: .infinity, minHeight: 48, alignment: .leading)
        .background(cardFill, in: RoundedRectangle(cornerRadius: 13))
        .overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(DT.hairlineSoft, lineWidth: 1))
        .shadow(color: DT.softShadow, radius: 2, y: 1)
        .help(compactHelp)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private var compactContent: some View {
        switch slot.availability {
        case .available(let remaining, let resetsAt, _):
            compactUsageRow(
                remaining: remaining,
                trailing: resetDetail(resetsAt)
            )
        case .stale(let remaining, let resetsAt, _):
            if let remaining {
                compactUsageRow(
                    remaining: remaining,
                    trailing: resetsAt.map { resetDetail($0) }
                        ?? localized("数据已过期 · Provider 未提供重置时间", locale: locale)
                )
            } else {
                Text("额度数据已过期")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(DT.amberText)
                    .padding(.top, 12)
            }
        case .unavailable:
            HStack(spacing: 8) {
                Text(slot.localizedUnavailableReason(language: model.appLanguage)
                    ?? localized("当前 Provider 版本暂不支持额度解析", locale: locale))
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textSecondary)
                    .lineLimit(2)
                Spacer(minLength: 4)
                Button("检查设置") { openSettings() }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
            }
            .padding(.top, 12)
        }
    }

    private func compactUsageRow(remaining: Double, trailing: String) -> some View {
        let remainingText = localizedFormat(
            "剩余 %lld%%",
            locale: locale,
            Int64(remaining.rounded())
        )
        return ViewThatFits(in: .horizontal) {
            HStack(alignment: .center, spacing: 9) {
                Text(remainingText)
                    .font(.system(size: 15, weight: .heavy))
                    .foregroundStyle(remaining < 50 ? DT.amberText : DT.textPrimary)
                    .monospacedDigit()
                    .fixedSize(horizontal: true, vertical: false)

                ProgressTrack(fraction: remaining / 100, color: compactTone)
                    .frame(minWidth: 48, idealWidth: 110, maxWidth: .infinity)
                    .frame(height: 5)

                Text(trailing)
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
            }

            VStack(alignment: .leading, spacing: 7) {
                HStack(alignment: .center, spacing: 9) {
                    Text(remainingText)
                        .font(.system(size: 15, weight: .heavy))
                        .foregroundStyle(remaining < 50 ? DT.amberText : DT.textPrimary)
                        .monospacedDigit()
                        .fixedSize(horizontal: true, vertical: false)

                    ProgressTrack(fraction: remaining / 100, color: compactTone)
                        .frame(minWidth: 48, maxWidth: .infinity)
                        .frame(height: 5)
                }

                Text(trailing)
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                    .frame(maxWidth: .infinity, alignment: .trailing)
            }
        }
        .padding(.top, 12)
    }

    @ViewBuilder
    private var content: some View {
        switch slot.availability {
        case .available(let remaining, let resetsAt, let capturedAt):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(localizedFormat(
                    "剩余 %lld%%",
                    locale: locale,
                    Int64(remaining.rounded())
                ))
                    .font(.system(size: 16, weight: .heavy))
                    .foregroundStyle(remaining < 50 ? DT.amberText : DT.textPrimary)
                Spacer(minLength: 2)
                Text(resetDetail(resetsAt))
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
            }
            .padding(.top, 8)
            ProgressTrack(
                fraction: remaining / 100,
                color: remaining < 50 ? Color.orange : DT.greenDot
            )
                .frame(height: 5)
                .padding(.top, 6)
            Text(capturedAt.map {
                localizedFormat(
                    "%lld 分钟前更新",
                    locale: locale,
                    Int64(max(0, Int(model.now.timeIntervalSince($0) / 60)))
                )
            } ?? localized("更新时间未提供", locale: locale))
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textFaint)
                .padding(.top, 6)
        case .stale(let remaining, let resetsAt, let capturedAt):
            Text(remaining.map {
                localizedFormat(
                    "上次记录剩余 %lld%%",
                    locale: locale,
                    Int64($0.rounded())
                )
            } ?? localized("额度数据已过期", locale: locale))
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(DT.amberText)
                .padding(.top, 8)
            Text([resetsAt.map { resetDetail($0) }, capturedAt.map {
                ZhFormat.relativeAgo(
                    model.now.timeIntervalSince($0),
                    language: model.appLanguage
                )
            }]
                .compactMap { $0 }.joined(separator: " · "))
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textFaint)
                .padding(.top, 6)
        case .unavailable:
            Text(slot.localizedUnavailableReason(language: model.appLanguage)
                ?? localized("当前 Provider 版本暂不支持额度解析", locale: locale))
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
        var components = [slot.providerDisplayName]
        if let plan = slot.displayPlanType { components.append(plan) }
        components.append(slot.localizedTitle(language: model.appLanguage))
        return components.joined(separator: " · ")
    }
    private var sourceLabel: String {
        let key = switch slot.source {
        case "oauth_usage": "OAuth 自动同步"
        case "codex_app_server": "Codex 自动同步"
        case "statusline": "Claude 对话同步"
        case "rollout_experimental": "本机 Session 同步"
        default: slot.source.replacingOccurrences(of: "_", with: " ")
        }
        return localized(key, locale: locale)
    }
    private var resetSourceLabel: String {
        let key = switch slot.resetSource {
        case "statusline": "官方 StatusLine"
        case "oauth_usage": "官方 OAuth"
        case "codex_app_server": "官方 Codex"
        case "rollout_experimental": "本机 Session 解析"
        case "local_estimate": "本机预计"
        case .some(let value): value.replacingOccurrences(of: "_", with: " ")
        case .none: "Provider 未提供"
        }
        return localized(key, locale: locale)
    }
    private func resetDetail(_ date: Date?) -> String {
        guard let date else {
            return localized("重置时间 · Provider 未提供", locale: locale)
        }
        return localizedFormat(
            "%@ · %@",
            locale: locale,
            resetText(date),
            resetSourceLabel
        )
    }
    private var cardFill: Color {
        if case .unavailable = slot.availability { return DT.cardFaint }
        return DT.cardMedium
    }
    private var compactRemaining: Double? {
        switch slot.availability {
        case .available(let remaining, _, _): remaining
        case .stale(let remaining, _, _): remaining
        case .unavailable: nil
        }
    }
    private var compactTone: Color {
        switch slot.availability {
        case .available(let remaining, _, _):
            remaining < 50 ? DT.amberDot : DT.greenDot
        case .stale:
            DT.amberDot
        case .unavailable:
            DT.textFaint
        }
    }
    private var compactIsAvailable: Bool {
        if case .available = slot.availability { return true }
        return false
    }
    private var compactStatus: String {
        let key = switch slot.availability {
        case .available:
            "可用"
        case .stale:
            "已过期"
        case .unavailable:
            "暂不可用"
        }
        return localized(key, locale: locale)
    }
    private var compactHelp: String {
        switch slot.availability {
        case .available(let remaining, let resetsAt, _):
            return localizedFormat(
                "%@，剩余 %lld%%，%@",
                locale: locale,
                title,
                Int64(remaining.rounded()),
                resetDetail(resetsAt)
            )
        case .stale(let remaining, _, _):
            let status = remaining.map {
                localizedFormat(
                    "上次记录剩余 %lld%%",
                    locale: locale,
                    Int64($0.rounded())
                )
            } ?? localized("额度数据已过期", locale: locale)
            return localizedFormat("%@，%@", locale: locale, title, status)
        case .unavailable:
            return localizedFormat(
                "%@，%@",
                locale: locale,
                title,
                slot.localizedUnavailableReason(language: model.appLanguage)
                    ?? localized("当前 Provider 版本暂不支持额度解析", locale: locale)
            )
        }
    }
    private func resetText(_ date: Date) -> String {
        let base = ZhFormat.resetTime(
            date,
            now: model.now,
            language: model.appLanguage
        )
        return Calendar.current.isDateInToday(date)
            ? localizedFormat("今天 %@", locale: locale, base)
            : base
    }
}
