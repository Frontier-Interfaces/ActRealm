import ActRealmKit
import SwiftUI

public struct HistoryCenterView: View {
    public init() {}

    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @State private var tasks: [RuntimeTaskHistoryRecord] = []
    @State private var selectedID: String?
    @State private var query = ""
    @State private var project = ""
    @State private var modelFilter = ""
    @State private var branch = ""
    @State private var provider = "all"
    @State private var status = "all"
    @State private var validation = "all"
    @State private var dateRange = "all"
    @State private var showInternalValidation = false
    @State private var loading = false
    @State private var error: String?
    @State private var pendingDelete: RuntimeTaskHistoryRecord?

    private var filteredTasks: [RuntimeTaskHistoryRecord] {
        let normalizedQuery = query.trimmingCharacters(in: .whitespacesAndNewlines)
            .localizedLowercase
        let normalizedProject = project.trimmingCharacters(in: .whitespacesAndNewlines)
            .localizedLowercase
        let normalizedModel = modelFilter.trimmingCharacters(in: .whitespacesAndNewlines)
            .localizedLowercase
        let normalizedBranch = branch.trimmingCharacters(in: .whitespacesAndNewlines)
            .localizedLowercase
        let cutoff = historyCutoff
        return tasks.filter { task in
            let searchable = [
                task.title,
                task.project,
                task.provider,
                task.model,
                task.branch
            ]
                .compactMap { $0?.localizedLowercase }
                .joined(separator: " ")
            return (normalizedQuery.isEmpty || searchable.contains(normalizedQuery))
                && (normalizedProject.isEmpty
                    || task.project?.localizedLowercase.contains(normalizedProject) == true)
                && (normalizedModel.isEmpty
                    || task.model?.localizedLowercase.contains(normalizedModel) == true)
                && (normalizedBranch.isEmpty
                    || task.branch?.localizedLowercase.contains(normalizedBranch) == true)
                && (provider == "all" || task.provider == provider)
                && statusMatches(task)
                && validationMatches(task)
                && (cutoff == nil || task.lastEventAt >= cutoff!)
                && (showInternalValidation || !HistoryRecordVisibility.isInternalValidation(
                    title: task.title,
                    project: task.project
                ))
        }
    }

    private var selected: RuntimeTaskHistoryRecord? {
        filteredTasks.first(where: { $0.id == selectedID })
            ?? filteredTasks.first
    }

    public var body: some View {
        VStack(spacing: 0) {
            historyHeader
            Rectangle().fill(DT.separator).frame(height: 1)
            HStack(spacing: 0) {
                filterRail
                    .frame(width: 238)
                Rectangle().fill(DT.separator).frame(width: 1)
                historyList
                    .frame(minWidth: 300, idealWidth: 360, maxWidth: 420)
                Rectangle().fill(DT.separator).frame(width: 1)
                detailPane
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .task { await load() }
        .onChange(of: model.client.snapshot.stats.eventCount) { _, _ in
            Task { await load(preserveSelection: true) }
        }
        .alert(
            localized("删除任务历史？", locale: locale),
            isPresented: Binding(
                get: { pendingDelete != nil },
                set: { if !$0 { pendingDelete = nil } }
            )
        ) {
            Button(localized("删除", locale: locale), role: .destructive) {
                guard let task = pendingDelete else { return }
                pendingDelete = nil
                Task { await delete(task) }
            }
            Button(localized("取消", locale: locale), role: .cancel) {}
        } message: {
            Text(localized(
                "只删除 ActRealm 本机任务记录与 Checkpoint 元数据；不会停止 Provider，也不会删除工作区文件、Commit 或分支。",
                locale: locale
            ))
        }
    }

    private var historyHeader: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(localized("历史中心", locale: locale))
                    .font(.system(size: 17, weight: .bold))
                    .foregroundStyle(DT.textStrong)
                Text(localized(
                    "活跃任务不会在这里重复；这里只保留已结束任务的本机结果",
                    locale: locale
                ))
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            Text(localizedFormat(
                "%lld 项结果",
                locale: locale,
                Int64(filteredTasks.count)
            ))
                .font(.system(size: 9.5, weight: .semibold))
                .foregroundStyle(DT.textWeak)
            Button {
                Task { await load(preserveSelection: true) }
            } label: {
                Label(localized("刷新", locale: locale), systemImage: "arrow.clockwise")
            }
            .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
            .disabled(loading)
        }
        .padding(.horizontal, 18)
        .frame(height: 58)
    }

    private var filterRail: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                filterTitle(localized("查找", locale: locale))
                TextField(localized("任务、项目、模型或分支", locale: locale), text: $query)
                    .textFieldStyle(.roundedBorder)

                filterTitle("Provider")
                Picker("Provider", selection: $provider) {
                    Text(localized("全部", locale: locale)).tag("all")
                    Text("Codex").tag("codex")
                    Text("Claude Code").tag("claude")
                }
                .labelsHidden()
                .pickerStyle(.segmented)

                filterPicker(
                    title: localized("日期", locale: locale),
                    selection: $dateRange,
                    items: [
                        ("all", localized("全部时间", locale: locale)),
                        ("7", localized("最近 7 天", locale: locale)),
                        ("30", localized("最近 30 天", locale: locale)),
                        ("90", localized("最近 90 天", locale: locale))
                    ]
                )

                Button(localized("清除筛选", locale: locale)) { resetFilters() }
                    .buttonStyle(.plain)
                    .font(.system(size: 9.5, weight: .semibold))
                    .foregroundStyle(DT.blueText)
            }
            .padding(14)
        }
        .background(DT.cardFaint)
    }

    private var historyList: some View {
        Group {
            if loading && tasks.isEmpty {
                ProgressView(localized("正在读取本机历史", locale: locale))
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if let error, tasks.isEmpty {
                emptyState(
                    icon: "exclamationmark.triangle",
                    title: localized("历史暂时无法读取", locale: locale),
                    detail: error
                )
            } else if filteredTasks.isEmpty {
                emptyState(
                    icon: "archivebox",
                    title: localized("没有匹配的历史任务", locale: locale),
                    detail: localized("调整筛选条件，或等待已完成任务离开活跃看板。", locale: locale)
                )
            } else {
                ScrollView {
                    LazyVStack(spacing: 7) {
                        ForEach(filteredTasks) { task in
                            Button { selectedID = task.id } label: {
                                historyRow(task)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(10)
                }
            }
        }
        .background(DT.cardStrong)
    }

    @ViewBuilder
    private var detailPane: some View {
        if let selected {
            HistoryTaskDetail(
                task: selected,
                client: model.client,
                onArchive: { Task { await archive(selected) } },
                onDelete: { pendingDelete = selected }
            )
            .id(selected.id)
        } else {
            emptyState(
                icon: "doc.text.magnifyingglass",
                title: localized("选择一项历史任务", locale: locale),
                detail: localized("查看最终结果、Review、Checkpoint 与安全事件摘要。", locale: locale)
            )
        }
    }

    private func historyRow(_ task: RuntimeTaskHistoryRecord) -> some View {
        let isSelected = selected?.id == task.id
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 7) {
                Circle()
                    .fill(task.provider == "claude" ? DT.amberDot : DT.blue)
                    .frame(width: 7, height: 7)
                Text(task.title ?? localized("未命名任务", locale: locale))
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(2)
                Spacer(minLength: 4)
                Image(systemName: task.status == "failed"
                    ? "xmark.circle.fill" : "checkmark.circle.fill")
                    .foregroundStyle(task.status == "failed" ? DT.redText : DT.greenText)
            }
            Text([task.project, task.model].compactMap { $0 }.joined(separator: " · "))
                .font(.system(size: 9))
                .foregroundStyle(DT.textWeak)
                .lineLimit(1)
            HStack(spacing: 6) {
                Text(historyDate(task.lastEventAt))
                if let branch = task.branch {
                    Text("·")
                    Text(branch).lineLimit(1)
                }
                Spacer()
                if task.archivedAt != nil {
                    Text(localized("已归档", locale: locale))
                }
            }
            .font(.system(size: 8.5))
            .foregroundStyle(DT.textFaint)
        }
        .padding(10)
        .background(isSelected ? DT.blueBg : DT.neutralChipBg,
                    in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 10, style: .continuous)
                .strokeBorder(isSelected ? DT.blueBadgeStroke : DT.neutralChipStroke, lineWidth: 1)
        )
    }

    private func filterTitle(_ title: String) -> some View {
        Text(title)
            .font(.system(size: 9, weight: .bold))
            .foregroundStyle(DT.textWeak)
            .textCase(.uppercase)
    }

    private func filterField(_ title: String, text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.system(size: 8.5)).foregroundStyle(DT.textFaint)
            TextField(title, text: text).textFieldStyle(.roundedBorder)
        }
    }

    private func filterPicker(
        title: String,
        selection: Binding<String>,
        items: [(String, String)]
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.system(size: 8.5)).foregroundStyle(DT.textFaint)
            Picker(title, selection: selection) {
                ForEach(items, id: \.0) { value, label in Text(label).tag(value) }
            }
            .labelsHidden()
            .frame(maxWidth: .infinity)
        }
    }

    private func emptyState(icon: String, title: String, detail: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: icon)
                .font(.system(size: 25, weight: .light))
                .foregroundStyle(DT.textFaint)
            Text(title).font(.system(size: 12, weight: .semibold)).foregroundStyle(DT.textPrimary)
            Text(detail)
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textWeak)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 280)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(24)
    }

    @MainActor
    private func load(preserveSelection: Bool = false) async {
        loading = true
        defer { loading = false }
        do {
            let response = try await model.client.taskHistory()
            guard response.schemaVersion == RuntimeTaskHistoryResponse.supportedSchemaVersion else {
                error = localized("历史数据版本暂不受支持", locale: locale)
                return
            }
            tasks = response.tasks
            if !preserveSelection || !tasks.contains(where: { $0.id == selectedID }) {
                selectedID = tasks.first?.id
            }
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    @MainActor
    private func archive(_ task: RuntimeTaskHistoryRecord) async {
        do {
            try await model.client.archiveTask(sessionId: task.id)
            await load(preserveSelection: true)
        } catch {
            self.error = error.localizedDescription
        }
    }

    @MainActor
    private func delete(_ task: RuntimeTaskHistoryRecord) async {
        do {
            try await model.client.deleteTaskHistory(sessionId: task.id)
            tasks.removeAll { $0.id == task.id }
            selectedID = filteredTasks.first?.id
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private var historyCutoff: UInt64? {
        guard let days = UInt64(dateRange) else { return nil }
        let now = UInt64(max(0, Date().timeIntervalSince1970 * 1_000))
        let interval = days * 86_400_000
        return now >= interval ? now - interval : 0
    }

    private func statusMatches(_ task: RuntimeTaskHistoryRecord) -> Bool {
        switch status {
        case "completed": task.status == "completed"
        case "failed": task.status == "failed"
        case "archived": task.archivedAt != nil
        default: true
        }
    }

    private func validationMatches(_ task: RuntimeTaskHistoryRecord) -> Bool {
        let value = task.validationState
        return switch validation {
        case "passed": value == "passed"
        case "failed": value == "failed"
        case "unverified": value != nil && !["passed", "failed"].contains(value!)
        case "none": value == nil
        default: true
        }
    }

    private func resetFilters() {
        query = ""
        project = ""
        modelFilter = ""
        branch = ""
        provider = "all"
        status = "all"
        validation = "all"
        dateRange = "all"
        showInternalValidation = false
    }

    private func historyDate(_ milliseconds: UInt64) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        return formatter.string(from: ZhFormat.date(fromMillis: milliseconds))
    }
}

enum HistoryRecordVisibility {
    static func isInternalValidation(title: String?, project: String?) -> Bool {
        let normalizedTitle = title?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .localizedLowercase ?? ""
        let normalizedProject = project?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .localizedLowercase ?? ""
        let knownFixtureProjects = [
            "claude-concurrent",
            "claude-nested-parent",
            "claude-ambiguous-parent",
            "claude-change-no-test",
            "claude-pass",
            "claude-fail",
            "codex-no-change",
        ]
        return normalizedTitle.contains("h2.4")
            || normalizedTitle.contains("真实验收")
            || normalizedTitle.contains("验收测试")
            || normalizedTitle.hasPrefix("for an actrealm approval smoke test")
            || normalizedProject.hasPrefix("h2-")
            || normalizedProject.hasPrefix("slugify-space-hyphen-fix-")
            || normalizedProject.hasPrefix("code-worktree-acceptance-")
            || knownFixtureProjects.contains(normalizedProject)
    }
}

private struct HistoryTaskDetail: View {
    @Environment(\.locale) private var locale
    let task: RuntimeTaskHistoryRecord
    @ObservedObject var client: RuntimeClient
    let onArchive: () -> Void
    let onDelete: () -> Void
    @State private var review: RuntimeTaskReviewSnapshot?
    @State private var checkpoints: [RuntimeTaskCheckpoint] = []
    @State private var loading = false
    @State private var error: String?
    @State private var jumpMessage: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                detailHeader
                facts
                if ProductScope.reviewEnabled || ProductScope.metadataCheckpointEnabled {
                    if loading {
                        ProgressView(localized("正在读取验收证据", locale: locale))
                            .controlSize(.small)
                    } else {
                        if ProductScope.reviewEnabled, hasReviewEvidence {
                            reviewCard
                        }
                        if ProductScope.metadataCheckpointEnabled, !checkpoints.isEmpty {
                            checkpointCard
                        }
                    }
                }
                if task.securityEventCount > 0 {
                    securityCard
                }
                if !ProductScope.reviewEnabled,
                   !ProductScope.metadataCheckpointEnabled,
                   task.securityEventCount == 0 {
                    noEvidenceCard
                }
                if let error {
                    Text(error).font(.system(size: 9.5)).foregroundStyle(DT.redText)
                }
                if let jumpMessage {
                    Text(jumpMessage).font(.system(size: 9.5)).foregroundStyle(DT.greenText)
                }
            }
            .padding(18)
        }
        .task { await loadDetails() }
    }

    private var detailHeader: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(task.title ?? localized("未命名任务", locale: locale))
                        .font(.system(size: 18, weight: .bold))
                        .foregroundStyle(DT.textStrong)
                        .textSelection(.enabled)
                    Text([providerName, task.model, task.project].compactMap { $0 }.joined(separator: " · "))
                        .font(.system(size: 10))
                        .foregroundStyle(DT.textWeak)
                }
                Spacer()
                if loading { ProgressView().controlSize(.small) }
                Button {
                    Task { await openOriginal() }
                } label: {
                    Label(localized(jumpButtonTitle, locale: locale), systemImage: "arrow.up.forward.app")
                }
                .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                .disabled(task.jumpCapability == "unsupported")
                if task.archivedAt == nil {
                    Button(action: onArchive) {
                        Label(localized("归档", locale: locale), systemImage: "archivebox")
                    }
                    .buttonStyle(ActionButtonStyle(kind: .secondary, compact: true))
                }
                Button(role: .destructive, action: onDelete) {
                    Label(localized("删除历史", locale: locale), systemImage: "trash")
                }
                .buttonStyle(.bordered)
            }
            Text(localized(
                "历史记录不会重新进入活跃看板；只有 Provider 产生新的真实 Turn 才会恢复。",
                locale: locale
            ))
                .font(.system(size: 9))
                .foregroundStyle(DT.textFaint)
        }
    }

    private var facts: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 150))], spacing: 7) {
            fact(localized("最终状态", locale: locale), statusText)
            fact(localized("最后活动", locale: locale), date(task.lastEventAt))
            if let branch = task.branch {
                fact(localized("Git 分支", locale: locale), branch)
            }
            fact(localized("验证结果", locale: locale), validationText(task.validationState))
            if ProductScope.metadataCheckpointEnabled, task.checkpointCount > 0 {
                fact("Checkpoint", String(task.checkpointCount))
            }
            fact(localized("跳转能力", locale: locale), localized(task.jumpLabel, locale: locale))
        }
    }

    private var noEvidenceCard: some View {
        detailCard(title: localized("验收证据", locale: locale), icon: "info.circle") {
            Text(localized(
                "这个任务没有额外的本机可验证结果；需要细节时请返回原 Agent。",
                locale: locale
            ))
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textWeak)
        }
    }

    private var reviewCard: some View {
        detailCard(title: "ActRealm Review", icon: "checkmark.seal") {
            if let review {
                Text(outcomeText(review.outcome.state))
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(review.outcome.state == "failed" ? DT.redText : DT.textPrimary)
                Text(repositoryText(review.repository))
                    .font(.system(size: 9.5))
                    .foregroundStyle(DT.textWeak)
                if review.validations.isEmpty {
                    Text(localized("未观察到结构化验证；不等于测试已通过", locale: locale))
                        .font(.system(size: 9))
                        .foregroundStyle(DT.amberText)
                } else {
                    ForEach(review.validations.suffix(4)) { run in
                        Label(
                            "\(run.kind) · \(validationText(run.state))",
                            systemImage: run.state == "passed" ? "checkmark.circle" : "info.circle"
                        )
                        .font(.system(size: 9))
                        .foregroundStyle(run.state == "passed" ? DT.greenText : DT.textWeak)
                    }
                }
            } else if !loading {
                Text(localized("Review 暂不可用", locale: locale))
                    .font(.system(size: 9.5)).foregroundStyle(DT.textFaint)
            }
        }
    }

    private var checkpointCard: some View {
        detailCard(title: localized("Checkpoint 与恢复", locale: locale), icon: "bookmark.square") {
            if checkpoints.isEmpty {
                Text(localized("尚未创建 Checkpoint", locale: locale))
                    .font(.system(size: 9.5)).foregroundStyle(DT.textFaint)
            } else {
                ForEach(checkpoints.prefix(5)) { checkpoint in
                    HStack {
                        Image(systemName: checkpoint.kind == "git_snapshot"
                            ? "point.3.connected.trianglepath.dotted" : "bookmark")
                            .foregroundStyle(DT.blueText)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(checkpoint.label ?? checkpoint.kind)
                                .font(.system(size: 9.5, weight: .semibold))
                            Text("\(date(checkpoint.createdAt)) · \(checkpoint.repository.branch ?? "—")")
                                .font(.system(size: 8.5)).foregroundStyle(DT.textFaint)
                        }
                        Spacer()
                    }
                }
                Text(localized(
                    "Checkpoint 验证是历史证据；恢复后的新 Turn 必须重新验证。",
                    locale: locale
                ))
                    .font(.system(size: 8.5))
                    .foregroundStyle(DT.amberText)
            }
        }
    }

    private var securityCard: some View {
        detailCard(title: localized("安全事件摘要", locale: locale), icon: "shield") {
            Text(task.securityEventCount == 0
                ? localized("未记录审批拒绝或失败事件", locale: locale)
                : localizedFormat(
                    "记录了 %lld 项审批或失败事件；原始命令与工具输出不在历史摘要中",
                    locale: locale,
                    Int64(task.securityEventCount)
                ))
                .font(.system(size: 9.5))
                .foregroundStyle(task.securityEventCount == 0 ? DT.textWeak : DT.amberText)
        }
    }

    private func detailCard<Content: View>(
        title: String,
        icon: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(title, systemImage: icon)
                .font(.system(size: 10.5, weight: .bold))
                .foregroundStyle(DT.textPrimary)
            content()
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(DT.neutralChipBg, in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(DT.neutralChipStroke, lineWidth: 1))
    }

    private func fact(_ label: String, _ value: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label).font(.system(size: 8.5)).foregroundStyle(DT.textFaint)
            Text(value).font(.system(size: 10, weight: .semibold)).foregroundStyle(DT.textPrimary)
                .lineLimit(1).textSelection(.enabled)
        }
        .padding(9)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(DT.cardMedium, in: RoundedRectangle(cornerRadius: 8))
    }

    @MainActor
    private func loadDetails() async {
        guard ProductScope.reviewEnabled || ProductScope.metadataCheckpointEnabled else {
            review = nil
            checkpoints = []
            loading = false
            error = nil
            return
        }
        loading = true
        defer { loading = false }
        let loadedReview: RuntimeTaskReviewSnapshot? = if ProductScope.reviewEnabled {
            try? await client.sessionReview(sessionId: task.id)
        } else {
            nil
        }
        let loadedCheckpoints: [RuntimeTaskCheckpoint]? = if ProductScope.metadataCheckpointEnabled {
            try? await client.sessionCheckpoints(sessionId: task.id)
        } else {
            []
        }
        review = loadedReview
        checkpoints = loadedCheckpoints ?? []
        error = (ProductScope.reviewEnabled && loadedReview == nil)
            || (ProductScope.metadataCheckpointEnabled && loadedCheckpoints == nil)
            ? localized("部分历史详情暂时无法读取", locale: locale)
            : nil
    }

    @MainActor
    private func openOriginal() async {
        let (response, error) = await client.jumpSession(task.id)
        if let response, response.success {
            jumpMessage = localized(response.label, locale: locale)
        } else {
            self.error = error ?? localized("无法返回原会话", locale: locale)
        }
    }

    private var providerName: String { task.provider == "claude" ? "Claude Code" : "Codex" }
    private var jumpButtonTitle: String {
        switch task.jumpCapability {
        case "app_only": "打开 Agent 应用"
        case "terminal": "返回终端会话"
        default: "返回原会话"
        }
    }
    private var hasReviewEvidence: Bool {
        guard let review else { return false }
        return review.repository.state == "available"
            || !review.validations.isEmpty
            || review.lastMeaningfulAction != nil
    }
    private var statusText: String {
        task.status == "failed" ? localized("失败", locale: locale) : localized("已完成", locale: locale)
    }
    private func validationText(_ value: String?) -> String {
        switch value {
        case "passed": localized("通过", locale: locale)
        case "failed": localized("失败", locale: locale)
        case nil: localized("未运行", locale: locale)
        default: localized("无法验证", locale: locale)
        }
    }
    private func outcomeText(_ value: String) -> String {
        switch value {
        case "completed": localized("任务已完成", locale: locale)
        case "failed": localized("任务失败", locale: locale)
        case "running": localized("任务仍在运行", locale: locale)
        default: localized("最终状态无法验证", locale: locale)
        }
    }
    private func repositoryText(_ repository: RuntimeReviewRepository) -> String {
        guard repository.state == "available" else {
            return localized("当前任务没有可验证的 Git 仓库", locale: locale)
        }
        let branch = repository.branch ?? "—"
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
        return localizedFormat(
            "%@ · %@ · %lld 个变更文件",
            locale: locale,
            branch,
            head,
            Int64(changed)
        )
    }
    private func date(_ milliseconds: UInt64) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter.string(from: ZhFormat.date(fromMillis: milliseconds))
    }
}
