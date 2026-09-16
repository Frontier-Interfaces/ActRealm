import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

private func activitySession(_ state: String = "tool_running", category: String? = "test", recovery: String? = nil) -> SessionRecord {
    SessionRecord(id: "task", provider: "codex", providerSessionId: "task", project: "fixture",
        title: "Task", model: nil, execState: state, approvalOwner: nil,
        activity: "Bash", activitySince: 1_000, planDone: nil, planTotal: nil,
        currentTool: "Bash", currentToolCategory: category, recoveryState: recovery, lastEventAt: 2_000)
}

private func activityAttention(_ kind: String, state: String = "open", acknowledged: UInt64? = nil) -> AttentionRecord {
    AttentionRecord(id: kind, sessionId: "task", provider: "codex", project: "fixture", requestId: nil,
        kind: kind, title: "Fixture", detail: nil, state: state, risk: "low", riskNotes: [],
        commandPreview: nil, expiresAt: nil, reminderAcknowledgedAt: acknowledged, createdAt: 1_000, resolution: nil)
}

@Suite struct TaskActivityPresentationTests {
    @Test func knownActionsLeadWithMeaningInsteadOfShellTransport() {
        let expected = [
            "test": "正在运行测试", "build": "正在构建项目", "code_check": "正在检查代码",
            "file_read": "正在读取文件", "file_edit": "正在编辑文件", "file_search": "正在查询项目",
            "network": "正在访问网络", "package": "正在处理依赖", "code_execution": "正在执行代码",
        ]
        for (category, text) in expected {
            let task = LaneTask(session: activitySession(category: category), openAttention: [])
            #expect(task.localizedCurrentAction(language: .simplifiedChinese) == text)
            #expect(!task.localizedCurrentAction(language: .english).contains("Bash"))
        }
        #expect(TaskActivityPresentation.action(category: "tool", tool: "MCP cua_repl.js", running: true, language: .simplifiedChinese) == "正在操作界面")
        #expect(TaskActivityPresentation.action(category: nil, tool: "view_image", running: true, language: .simplifiedChinese) == "正在查看图像")
        #expect(TaskActivityPresentation.action(category: nil, tool: "Bash", running: true, language: .simplifiedChinese) == "正在执行命令")
    }

    @Test func waitingAndDecisionPhasesOverrideAnOldToolLabel() {
        for (kind, phase, label) in [
            ("approval", "open", "等待批准"), ("native_approval", "open", "等待原界面批准"),
            ("question", "open", "等待回答"), ("approval", "committing", "正在提交决定"),
            ("approval", "decision_sent", "已处理，等待 Agent 确认"),
        ] {
            let task = LaneTask(session: activitySession("awaiting_approval"), openAttention: [activityAttention(kind, state: phase)])
            #expect(task.localizedCurrentAction(language: .simplifiedChinese) == label)
            #expect(!task.localizedCurrentAction(language: .simplifiedChinese).contains("测试"))
        }
    }

    @Test func completionConfirmationAndSourceLossRemainDistinct() {
        let finished = activitySession("response_finished")
        let pending = LaneTask(session: finished, openAttention: [activityAttention("completion")])
        #expect(pending.awaitingCompletionConfirmation)
        #expect(pending.localizedState(language: .simplifiedChinese) == "完成待确认")
        let acknowledged = LaneTask(session: finished, openAttention: [], visibleAttention: [activityAttention("completion", acknowledged: 3_000)])
        #expect(acknowledged.localizedState(language: .simplifiedChinese) == "已确认完成")
        let ended = LaneTask(session: finished, openAttention: [])
        #expect(ended.localizedCurrentAction(language: .simplifiedChinese) == "本轮已完成")
        let lost = LaneTask(session: activitySession(recovery: "lost_control"), openAttention: [])
        #expect(lost.localizedCurrentAction(language: .simplifiedChinese) == "等待新事件")
        let failed = LaneTask(session: activitySession("thinking"), openAttention: [activityAttention("error")])
        #expect(failed.status == .failed)
        #expect(failed.localizedCurrentAction(language: .simplifiedChinese) == "任务失败")
    }

    @Test func finishedToolRetainsStartCategoryButDoesNotInventPassingTests() {
        let start = RuntimeTimelineEvent(eventId: "start", provider: "codex", kind: "tool.started",
            toolName: "Bash", toolCategory: "test", toolTarget: "quota_tests.rs", toolCallId: "call", validationStatus: "running",
            turnId: "turn", occurredAt: 1_000, ingestSequence: 1, contextAvailability: "anchor_missing")
        let end = RuntimeTimelineEvent(eventId: "end", provider: "codex", kind: "tool.completed",
            toolName: "Bash", toolCategory: "shell", toolCallId: "call",
            turnId: "turn", occurredAt: 2_000, ingestSequence: 2, contextAvailability: "anchor_missing")
        let orphan = TaskWorkflowPresentation.items([start])[0]
        #expect(TaskWorkflowPresentation.title(for: orphan, locale: AppLanguage.simplifiedChinese.locale) == "运行测试 · quota_tests.rs · 未收到结束事件")
        let live = TaskWorkflowPresentation.items([start], activeToolName: "Bash")[0]
        #expect(TaskWorkflowPresentation.title(for: live, locale: AppLanguage.simplifiedChinese.locale) == "运行测试 · quota_tests.rs · 执行中")
        let items = TaskWorkflowPresentation.items([start, end])
        #expect(items.count == 1)
        #expect(items[0].toolCategory == "test")
        #expect(items[0].toolName == "Bash")
        #expect(items[0].toolTarget == "quota_tests.rs")
        #expect(TaskWorkflowPresentation.title(for: items[0], locale: AppLanguage.simplifiedChinese.locale) == "运行测试 · quota_tests.rs · 已执行，结果无法验证")
    }

    @Test func distinctFileTargetsAndValidationFailuresAreNotCollapsedAway() {
        let events = ["first.swift", "second.swift"].enumerated().map { index, name in
            RuntimeTimelineEvent(eventId: name, provider: "claude", kind: "tool.completed", toolName: "Bash",
                toolCategory: "file_read", toolTarget: name, occurredAt: UInt64(index + 1),
                ingestSequence: UInt64(index + 1), contextAvailability: "anchor_missing")
        }
        let items = TaskWorkflowPresentation.items(events)
        #expect(items.count == 2)
        #expect(items.map(\.toolTarget) == ["first.swift", "second.swift"])
        let failed = RuntimeTimelineEvent(eventId: "failed", provider: "codex", kind: "tool.completed",
            toolName: "Bash", toolCategory: "code_check", validationStatus: "failed",
            occurredAt: 3, ingestSequence: 3, contextAvailability: "anchor_missing")
        let check = TaskWorkflowPresentation.items([failed])[0]
        #expect(TaskWorkflowPresentation.isFailure(check))
        #expect(TaskWorkflowPresentation.title(for: check, locale: AppLanguage.simplifiedChinese.locale) == "检查代码 · 失败")
    }
}
