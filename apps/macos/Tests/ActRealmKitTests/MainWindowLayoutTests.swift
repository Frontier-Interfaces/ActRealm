import CoreGraphics
import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct MainWindowLayoutTests {
    @Test func quotaColumnKeepsItsMinimumAndEveryColumnGrowsOnWideWindows() {
        let minimum = WorkspaceColumnLayout.resolve(containerWidth: 1128)
        #expect(minimum.quota == WorkspaceColumnLayout.minimumQuotaWidth)
        #expect(minimum.tasks >= 480)

        let wide = WorkspaceColumnLayout.resolve(containerWidth: 1968)
        #expect(wide.quota > minimum.quota)
        #expect(wide.outbox > minimum.outbox)
        #expect(wide.tasks > minimum.tasks)

        let expected = 1968 - WorkspaceColumnLayout.gap * 2
        #expect(abs(wide.outbox + wide.tasks + wide.quota - expected) < 0.001)
    }

    @Test func englishQuotaColumnGetsRoomForLongerUsageLabels() {
        let english = WorkspaceColumnLayout.resolve(
            containerWidth: 1128,
            language: .english
        )
        #expect(english.quota == WorkspaceColumnLayout.minimumEnglishQuotaWidth)
        #expect(english.quota > WorkspaceColumnLayout.minimumQuotaWidth)
        #expect(english.tasks >= 420)

        let expected = 1128 - WorkspaceColumnLayout.gap * 2
        #expect(abs(english.outbox + english.tasks + english.quota - expected) < 0.001)
    }

    @Test func historyHidesOnlyExplicitInternalValidationFixturesByDefault() {
        #expect(HistoryRecordVisibility.isInternalValidation(
            title: "H2.4 C01 真实验收",
            project: "project"
        ))
        #expect(HistoryRecordVisibility.isInternalValidation(
            title: "Ordinary task",
            project: "h2-c01-codex-pass"
        ))
        #expect(HistoryRecordVisibility.isInternalValidation(
            title: "Claude Code H2.4 L02 验收测试",
            project: "claude-fail"
        ))
        #expect(HistoryRecordVisibility.isInternalValidation(
            title: "Slugify 空格转连字符修复",
            project: "slugify-space-hyphen-fix-f9f5c4"
        ))
        #expect(!HistoryRecordVisibility.isInternalValidation(
            title: "修复用户验收流程",
            project: "ActRealm-Cloud"
        ))
    }

    @Test func englishMenuBarPopoverGetsRoomForLongerLabels() {
        #expect(MenuBarPopoverLayout.width(language: .simplifiedChinese) == 330)
        #expect(MenuBarPopoverLayout.width(language: .english) == 360)
    }

    @Test @MainActor func demoQuotaModeChangeKeepsSnapshotContent() {
        let suite = "ActRealmMainWindowLayoutTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        let quotaCount = model.derived.quotaSlots.count
        #expect(quotaCount > 0)

        model.updateUISettings { $0.quotaDisplayMode = .compact }
        #expect(model.uiSettings.quotaDisplayMode == .compact)
        #expect(model.derived.quotaSlots.count == quotaCount)

        model.updateUISettings { $0.tokenUsageDisplayMode = .hidden }
        #expect(model.uiSettings.tokenUsageDisplayMode == .hidden)
        #expect(model.derived.quotaSlots.count == quotaCount)
    }




    @Test func localTaskWorkflowUsesSanitizedRuntimeFacts() {
        let plan = RuntimeTimelineEvent(
            eventId: "plan-event",
            provider: "codex",
            kind: "plan.updated",
            toolName: nil,
            toolTarget: nil,
            riskLevel: nil,
            planStepCount: 4,
            turnId: nil,
            outboxId: nil,
            occurredAt: 1_787_000_000_000,
            ingestSequence: 10,
            contextAvailability: "anchor_missing"
        )
        let tool = RuntimeTimelineEvent(
            eventId: "tool-event",
            provider: "codex",
            kind: "tool.started",
            toolName: "apply_patch",
            toolTarget: "LanesSection.swift",
            riskLevel: nil,
            planStepCount: nil,
            turnId: nil,
            outboxId: nil,
            occurredAt: 1_787_000_001_000,
            ingestSequence: 11,
            contextAvailability: "anchor_missing"
        )

        #expect(TaskWorkflowPresentation.title(
            for: plan,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "任务流程已更新 · 4 个步骤")
        #expect(TaskWorkflowPresentation.title(
            for: tool,
            locale: AppLanguage.english.locale
        ) == "Started apply_patch")
        let unnamedTool = RuntimeTimelineEvent(
            eventId: "unnamed-tool-event",
            provider: "codex",
            kind: "tool.started",
            toolName: nil,
            toolTarget: nil,
            riskLevel: nil,
            planStepCount: nil,
            turnId: nil,
            outboxId: nil,
            occurredAt: 1_787_000_002_000,
            ingestSequence: 12,
            contextAvailability: "anchor_missing"
        )
        #expect(TaskWorkflowPresentation.title(
            for: unnamedTool,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "开始使用工具")
        #expect(TaskWorkflowPresentation.icon(for: "plan.updated") == "checklist")
        #expect(!TaskWorkflowPresentation.isFailure("tool.started"))
        #expect(TaskWorkflowPresentation.isFailure("tool.failed"))
    }

    @Test func completedPlanProgressDoesNotClaimItIsStillRunning() {
        #expect(TaskPlanPresentation.progressText(
            done: 3,
            total: 4,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "3/4（进行中）")
        #expect(TaskPlanPresentation.progressText(
            done: 4,
            total: 4,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "4/4（已完成）")
        #expect(TaskPlanPresentation.progressText(
            done: 4,
            total: 4,
            locale: AppLanguage.english.locale
        ) == "4/4 (completed)")

        let steps = [
            PlanStepRecord(
                id: "1",
                text: "Inspect",
                detail: nil,
                status: "in_progress",
                source: "codex_turn_plan"
            )
        ]
        #expect(TaskPlanPresentation.compactText(
            done: 0,
            total: 8,
            steps: steps,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "当前第 1/8 步 · 已完成 0")
        #expect(TaskPlanPresentation.compactText(
            done: 0,
            total: 8,
            steps: steps,
            locale: AppLanguage.english.locale
        ) == "Step 1/8 · 0 completed")
    }

    @Test func expandedWorkflowKeepsAStableBoundedHeight() {
        let events = (0 ..< 1_150).map { index in
            RuntimeTimelineEvent(
                eventId: "event-\(index)",
                provider: "codex",
                kind: "tool.completed",
                toolName: nil,
                toolTarget: nil,
                riskLevel: nil,
                planStepCount: nil,
                turnId: nil,
                outboxId: nil,
                occurredAt: UInt64(index),
                ingestSequence: UInt64(index),
                contextAvailability: "anchor_missing"
            )
        }

        let visible = TaskWorkflowPresentation.visibleEvents(events)

        #expect(visible.count == TaskWorkflowPresentation.maximumVisibleEvents)
        #expect(visible.first?.eventId == "event-150")
        #expect(visible.last?.eventId == "event-1149")
    }

    @Test func workflowCombinesToolLifecycleIntoOneUsefulRow() {
        let started = RuntimeTimelineEvent(
            eventId: "started",
            provider: "codex",
            kind: "tool.started",
            toolName: "apply_patch",
            toolTarget: "LanesSection.swift",
            riskLevel: nil,
            planStepCount: nil,
            turnId: "turn",
            outboxId: nil,
            occurredAt: 1_000,
            ingestSequence: 1,
            contextAvailability: "anchor_missing"
        )
        let completed = RuntimeTimelineEvent(
            eventId: "completed",
            provider: "codex",
            kind: "tool.completed",
            toolName: "apply_patch",
            toolTarget: nil,
            riskLevel: nil,
            planStepCount: nil,
            turnId: "turn",
            outboxId: nil,
            occurredAt: 2_250,
            ingestSequence: 2,
            contextAvailability: "anchor_missing"
        )

        let items = TaskWorkflowPresentation.items([started, completed])

        #expect(items.count == 1)
        #expect(items[0].durationMillis == 1_250)
        #expect(TaskWorkflowPresentation.title(
            for: items[0],
            locale: AppLanguage.simplifiedChinese.locale
        ) == "编辑文件 · LanesSection.swift · 已完成")
        #expect(TaskWorkflowPresentation.durationText(
            for: items[0],
            locale: AppLanguage.english.locale
        ) == "1.2 sec")

        let orphaned = TaskWorkflowPresentation.items([started])
        #expect(TaskWorkflowPresentation.title(
            for: orphaned[0],
            locale: AppLanguage.simplifiedChinese.locale
        ) == "编辑文件 · LanesSection.swift · 未收到结束事件")
        let active = TaskWorkflowPresentation.items(
            [started],
            activeToolName: "apply_patch"
        )
        #expect(TaskWorkflowPresentation.title(
            for: active[0],
            locale: AppLanguage.simplifiedChinese.locale
        ) == "编辑文件 · LanesSection.swift · 执行中")
    }

    @Test func workflowPairsParallelSameNameToolsByInvocationIdentity() {
        func event(
            _ id: String,
            kind: String,
            callID: String,
            target: String?,
            at: UInt64
        ) -> RuntimeTimelineEvent {
            RuntimeTimelineEvent(
                eventId: id,
                provider: "codex",
                kind: kind,
                toolName: "Bash",
                toolTarget: target,
                toolCallId: callID,
                turnId: "turn",
                occurredAt: at,
                ingestSequence: at,
                contextAvailability: "anchor_missing"
            )
        }
        let items = TaskWorkflowPresentation.items([
            event("a-start", kind: "tool.started", callID: "a", target: "a.swift", at: 1_000),
            event("b-start", kind: "tool.started", callID: "b", target: "b.swift", at: 2_000),
            event("b-end", kind: "tool.completed", callID: "b", target: nil, at: 3_000),
            event("a-end", kind: "tool.failed", callID: "a", target: nil, at: 5_000),
        ])

        #expect(items.count == 2)
        #expect(items[0].toolTarget == "a.swift")
        #expect(items[0].durationMillis == 4_000)
        #expect(items[0].event.kind == "tool.failed")
        #expect(items[1].toolTarget == "b.swift")
        #expect(items[1].durationMillis == 1_000)
    }

    @Test func workflowEmptyStateDoesNotInventUnsupportedFacts() {
        let locale = AppLanguage.simplifiedChinese.locale
        #expect(TaskWorkflowPresentation.emptyText(
            capability: .supported,
            running: true,
            locale: locale
        ) == "等待当前 Turn 的首个工具事件")
        #expect(TaskWorkflowPresentation.emptyText(
            capability: .supported,
            running: false,
            locale: locale
        ) == "当前 Turn 没有工具调用")
        #expect(TaskWorkflowPresentation.emptyText(
            capability: .unsupported,
            running: true,
            locale: locale
        ) == "Provider 不支持工具工作流")
    }

    @Test func workflowCollapsesRoutineBashButKeepsLongAndFailedCommands() {
        func event(_ id: String, kind: String, at: UInt64) -> RuntimeTimelineEvent {
            RuntimeTimelineEvent(
                eventId: id,
                provider: "claude",
                kind: kind,
                toolName: "Bash",
                toolTarget: nil,
                riskLevel: nil,
                planStepCount: nil,
                turnId: "turn",
                outboxId: nil,
                occurredAt: at,
                ingestSequence: at,
                contextAvailability: "anchor_missing"
            )
        }

        let items = TaskWorkflowPresentation.items([
            event("short-1-start", kind: "tool.started", at: 1_000),
            event("short-1-end", kind: "tool.completed", at: 2_000),
            event("short-2-start", kind: "tool.started", at: 3_000),
            event("short-2-end", kind: "tool.completed", at: 5_000),
            event("long-start", kind: "tool.started", at: 6_000),
            event("long-end", kind: "tool.completed", at: 18_000),
            event("failed-start", kind: "tool.started", at: 19_000),
            event("failed-end", kind: "tool.failed", at: 20_000),
        ])

        #expect(items.count == 3)
        #expect(items[0].occurrenceCount == 2)
        #expect(items[0].durationMillis == 3_000)
        #expect(TaskWorkflowPresentation.title(
            for: items[0],
            locale: AppLanguage.simplifiedChinese.locale
        ) == "执行命令 × 2 · 已完成")
        #expect(items[1].durationMillis == 12_000)
        #expect(items[2].event.kind == "tool.failed")
    }

    @Test func workflowShowsSemanticCategoryWithoutHidingTheExactTool() {
        let event = RuntimeTimelineEvent(
            eventId: "semantic-test",
            provider: "codex",
            kind: "tool.completed",
            toolName: "Bash",
            toolCategory: "test",
            validationStatus: "unverifiable",
            occurredAt: 2_000,
            ingestSequence: 2,
            contextAvailability: "anchor_missing"
        )
        let item = TaskWorkflowPresentation.items([event])[0]
        #expect(TaskWorkflowPresentation.title(
            for: item,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "运行测试 · 已执行，结果无法验证")
        #expect(TaskWorkflowPresentation.icon(for: item) == "questionmark.circle.fill")
        #expect(!TaskWorkflowPresentation.isFailure(item))
        #expect(TaskWorkflowPresentation.currentActionLabel(
            "code_execution",
            locale: AppLanguage.simplifiedChinese.locale
        ) == "正在执行代码")
        #expect(TaskWorkflowPresentation.currentActionLabel(
            "test",
            locale: AppLanguage.simplifiedChinese.locale
        ) == "正在运行测试")

        let failed = RuntimeTimelineEvent(
            eventId: "semantic-test-failed",
            provider: "claude",
            kind: "tool.completed",
            toolName: "Bash",
            toolCategory: "test",
            validationStatus: "failed",
            occurredAt: 3_000,
            ingestSequence: 3,
            contextAvailability: "anchor_missing"
        )
        let failedItem = TaskWorkflowPresentation.items([failed])[0]
        #expect(TaskWorkflowPresentation.title(
            for: failedItem,
            locale: AppLanguage.english.locale
        ) == "Run tests · Failed")
        #expect(TaskWorkflowPresentation.icon(for: failedItem) == "exclamationmark.triangle.fill")
        #expect(TaskWorkflowPresentation.isFailure(failedItem))
        #expect(TaskWorkflowPresentation.categoryLabel(
            "code_execution",
            locale: AppLanguage.simplifiedChinese.locale
        ) == "代码执行")
        #expect(TaskWorkflowPresentation.categoryLabel(
            "interaction",
            locale: AppLanguage.english.locale
        ) == "Interface interaction")
    }

    @Test func runningWorkflowMakesSilenceExplicitWithoutClaimingFailure() {
        #expect(TaskWorkflowFreshnessPresentation.text(
            age: 3,
            running: true,
            language: .simplifiedChinese,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "实时")
        #expect(TaskWorkflowFreshnessPresentation.text(
            age: 130,
            running: true,
            language: .simplifiedChinese,
            locale: AppLanguage.simplifiedChinese.locale
        ) == "2 分 10 秒 无新事件")
        #expect(!TaskWorkflowFreshnessPresentation.isQuiet(
            age: 300,
            running: false
        ))
    }

}
