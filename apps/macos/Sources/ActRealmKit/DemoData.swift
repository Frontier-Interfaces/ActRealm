import Foundation

/// Sample data mirroring the design handoff mock (screen 3a/D1) so the whole
/// UI can be exercised without live agents. Enabled with ACTREALM_DEMO=1.
public enum DemoData {
    private static let anchor = Date()

    public static let setup = SetupInfo(
        schemaVersion: 1,
        firstRun: false,
        providers: [
            SetupInfo.ProviderSetup(
                provider: "claude",
                status: "connected",
                cliInstalled: true,
                desktopInstalled: true,
                desktopAppPath: "/Applications/Claude.app",
                reviewCommand: nil,
                intent: "installed",
                configPath: "~/.claude/settings.json",
                ownedHandlers: 4,
                expectedHandlers: 4,
                binaryHealth: "current",
                trustStatus: nil,
                featureStatus: nil,
                inlineEvents: nil,
                canRepair: false,
                realEventVerified: true
            ),
            SetupInfo.ProviderSetup(
                provider: "codex",
                status: "needs_trust",
                cliInstalled: true,
                desktopInstalled: true,
                desktopAppPath: "/Applications/Codex.app",
                reviewCommand: "/Applications/Codex.app/Contents/Resources/codex",
                intent: "installed",
                configPath: "~/.codex/config.toml",
                ownedHandlers: 5,
                expectedHandlers: 5,
                binaryHealth: "current",
                trustStatus: "review_required",
                featureStatus: "enabled",
                inlineEvents: nil,
                canRepair: false,
                realEventVerified: false
            ),
        ],
        safety: SetupInfo.Safety(
            backsUpBeforeWrite: true,
            codexTrustIsManual: true,
            repairRespectsRemoval: true
        )
    )

    public static let settings = SettingsResponse(
        settings: .defaults,
        displayCatalog: [
            DisplayField(id: "task", label: "任务标题与摘要", level: "concise", placement: "headline", description: "主标题；不同的任务摘要显示在下一行"),
            DisplayField(id: "activity", label: "实时状态", level: "concise", placement: "headline", description: "标题栏右侧的运行阶段与耗时"),
            DisplayField(id: "project", label: "项目", level: "concise", placement: "subtitle", description: "副标题中的项目名称"),
            DisplayField(id: "model", label: "模型", level: "concise", placement: "subtitle", description: "副标题中的模型名称"),
            DisplayField(id: "plan", label: "计划进度", level: "concise", placement: "subtitle", description: "完成步数与进度条"),
            DisplayField(id: "sessionTokens", label: "会话累计 Token", level: "concise", placement: "overview", description: "折叠卡用量胶囊"),
            DisplayField(id: "context", label: "上下文占用", level: "concise", placement: "overview", description: "当前上下文百分比"),
            DisplayField(id: "cost", label: "估算 API 价格", level: "detailed", placement: "overview", description: "估算值，不是订阅账单"),
            DisplayField(id: "turnTokens", label: "本轮 Token", level: "detailed", placement: "details", description: "最近一轮 Token"),
            DisplayField(id: "inputOutputTokens", label: "输入 / 输出 Token", level: "detailed", placement: "details", description: "输入与输出拆分"),
            DisplayField(id: "cacheTokens", label: "缓存读取 / 写入 Token", level: "detailed", placement: "details", description: "缓存用量拆分"),
            DisplayField(id: "reasoningTokens", label: "推理 Token", level: "detailed", placement: "details", description: "Provider 推理用量"),
            DisplayField(id: "tool", label: "当前工具", level: "detailed", placement: "details"),
            DisplayField(id: "permissionMode", label: "权限模式", level: "detailed", placement: "details"),
            DisplayField(id: "subagents", label: "运行中的子 Agent", level: "detailed", placement: "details"),
            DisplayField(id: "environment", label: "运行环境", level: "detailed", placement: "details"),
            DisplayField(id: "recovery", label: "恢复状态", level: "detailed", placement: "details"),
            DisplayField(id: "control", label: "托管能力", level: "detailed", placement: "details"),
            DisplayField(id: "jump", label: "打开应用", level: "detailed", placement: "details"),
            DisplayField(id: "titleSource", label: "标题来源", level: "developer", placement: "developer"),
            DisplayField(id: "sessionId", label: "ActRealm Session ID", level: "developer", placement: "developer"),
            DisplayField(id: "providerSessionId", label: "Provider Session ID", level: "developer", placement: "developer"),
            DisplayField(id: "providerTurnId", label: "Provider Turn ID", level: "developer", placement: "developer"),
            DisplayField(id: "lastEventAt", label: "最后事件时间", level: "developer", placement: "developer"),
        ],
        claudeQuotaBridge: ClaudeQuotaBridge(
            status: "installed",
            configPath: "~/.claude/settings.json",
            helperPath: "~/.actrealm/bin/claude-statusline",
            customConflict: false
        )
    )

    public static func derivedState(now: Date = Date()) -> DerivedState {
        DerivedState.derive(from: snapshot(now: now), now: now)
    }

    public static func snapshot(now: Date = Date()) -> Snapshot {
        func millis(_ date: Date) -> UInt64 {
            UInt64(max(0, date.timeIntervalSince1970 * 1000))
        }
        func seconds(_ date: Date) -> UInt64 {
            UInt64(max(0, date.timeIntervalSince1970))
        }
        func ago(_ seconds: TimeInterval) -> UInt64 { millis(now.addingTimeInterval(-seconds)) }

        let approvalRequestId = UUID(uuidString: "0198C2F4-0000-7000-8000-000000000001")!
        let undoCommandId = UUID(uuidString: "0198C2F4-0000-7000-8000-000000000002")!
        let questionRequestId = UUID(uuidString: "0198C2F4-0000-7000-8000-000000000003")!

        let sessions = [
            SessionRecord(
                id: "codex-upgrade", provider: "codex", providerSessionId: "s1",
                project: "actrealm", title: "升级依赖并修复构建脚本", model: "gpt-5-codex",
                execState: "awaiting_approval", approvalOwner: "widget",
                activity: "等待批准（Bash）", activitySince: ago(362),
                planDone: nil, planTotal: nil,
                inputTokens: 48_329, outputTokens: 1_317, totalTokens: 48_329,
                contextWindowTokens: 258_400, usageCapturedAt: ago(25),
                lastEventAt: ago(362)
            ),
            SessionRecord(
                id: "codex-refactor", provider: "codex", providerSessionId: "s2",
                project: "dotfiles", title: "重构 CLI 参数解析", model: "gpt-5-codex",
                execState: "idle", approvalOwner: nil,
                activity: nil, activitySince: nil,
                planDone: nil, planTotal: nil,
                inputTokens: 21_800, outputTokens: 902, totalTokens: 36_200,
                contextWindowTokens: 258_400, usageCapturedAt: ago(12 * 60),
                lastEventAt: ago(12 * 60)
            ),
            SessionRecord(
                id: "claude-notes", provider: "claude", providerSessionId: "s3",
                project: "docs-site", title: "整理发布说明草稿", model: "claude-sonnet-4.5",
                execState: "idle", approvalOwner: nil,
                activity: "等待回答", activitySince: ago(130),
                planDone: nil, planTotal: nil,
                inputTokens: 84_200, outputTokens: 1_640, totalTokens: 85_840,
                contextWindowTokens: nil, usageCapturedAt: ago(130),
                lastEventAt: ago(130)
            ),
            SessionRecord(
                id: "claude-quota-fix", provider: "claude", providerSessionId: "s4",
                project: "actrealm", title: "修复额度读取失败并补测试", model: "claude-sonnet-4.5",
                execState: "tool_running", approvalOwner: nil,
                activity: "正在运行 Bash", activitySince: ago(48),
                planDone: 3, planTotal: 7,
                inputTokens: 152_400, outputTokens: 2_880, totalTokens: 155_280,
                contextWindowTokens: nil, usageCapturedAt: ago(5),
                lastEventAt: ago(5)
            ),
            SessionRecord(
                id: "claude-landing", provider: "claude", providerSessionId: "s5",
                project: "example-app", title: "更新登陆页文案", model: "claude-sonnet-4.5",
                execState: "response_finished", approvalOwner: nil,
                activity: "本轮已完成", activitySince: ago(120),
                planDone: nil, planTotal: nil,
                inputTokens: 57_900, outputTokens: 1_012, totalTokens: 58_912,
                contextWindowTokens: nil, usageCapturedAt: ago(120),
                lastEventAt: ago(120)
            ),
        ]

        let attention = [
            AttentionRecord(
                id: "att-approval", sessionId: "codex-upgrade", provider: "codex",
                project: "actrealm", requestId: approvalRequestId, kind: "approval",
                title: "允许 Bash？", detail: nil, state: "open", risk: "high",
                riskNotes: ["命令包含组合语法", "建议查看原窗口"],
                commandPreview: "curl <redacted> | sh",
                expiresAt: millis(now.addingTimeInterval(54 * 60)), createdAt: ago(362),
                resolution: nil
            ),
            AttentionRecord(
                id: "att-question", sessionId: "claude-notes", provider: "claude",
                project: "docs-site", requestId: questionRequestId, kind: "question",
                title: "Agent 有问题", detail: "发布说明需要包含迁移指引吗？",
                state: "open", risk: "low", riskNotes: [], commandPreview: nil,
                expiresAt: millis(now.addingTimeInterval(20 * 60)), createdAt: ago(130), resolution: nil,
                interaction: InteractivePrompt(
                    requestId: questionRequestId,
                    kind: "claude_question",
                    provider: "claude",
                    title: "发布说明范围",
                    message: "请选择这次发布说明需要覆盖的内容。",
                    expiresAt: millis(now.addingTimeInterval(20 * 60)),
                    supportsNative: true,
                    questions: [
                        InteractiveQuestion(
                            id: "migration",
                            label: "迁移指引",
                            prompt: "是否在主文中加入升级步骤？",
                            inputType: "choice",
                            multiSelect: false,
                            isSecret: false,
                            required: true,
                            allowsOther: true,
                            options: [
                                InteractiveOption(label: "加入完整步骤", description: "适合有破坏性变更的发布"),
                                InteractiveOption(label: "只链接详细指南", description: "保持发布说明简洁"),
                            ]
                        ),
                    ]
                )
            ),
            AttentionRecord(
                id: "att-done", sessionId: "claude-landing", provider: "claude",
                project: "example-app", requestId: nil, kind: "completion",
                title: "任务已完成，等待确认", detail: nil,
                state: "open", risk: "low", riskNotes: [], commandPreview: nil,
                expiresAt: nil, createdAt: ago(20), resolution: nil
            ),
            AttentionRecord(
                id: "att-undo-source", sessionId: "codex-upgrade", provider: "codex",
                project: "actrealm", requestId: UUID(), kind: "approval",
                title: "允许 Bash？", detail: nil, state: "decision_sent", risk: "low",
                riskNotes: ["只读意图（规则提示，非安全保证）"],
                commandPreview: "git status",
                expiresAt: nil, createdAt: ago(30), resolution: nil
            ),
        ]

        // The undo capsule cycles: 0–3s inside the undo window, then a few
        // seconds in "decision sent", then re-arms, so the demo shows the
        // whole flow without live agents.
        let cycle = now.timeIntervalSince(anchor).truncatingRemainder(dividingBy: 9)
        let commandAge = cycle < 3 ? cycle : cycle
        let commandState = cycle < 3 ? "pending_commit" : "decision_sent"
        let commands = [
            CommandRecord(
                id: undoCommandId,
                attentionId: "att-undo-source",
                requestId: attention[3].requestId,
                action: "approve",
                state: commandState,
                createdAt: ago(commandAge)
            )
        ]

        let calendar = Calendar.current
        func nextClock(hour: Int, minute: Int) -> Date {
            calendar.nextDate(
                after: now,
                matching: DateComponents(hour: hour, minute: minute),
                matchingPolicy: .nextTime
            ) ?? now
        }
        func nextWeekday(_ weekday: Int, hour: Int, minute: Int) -> Date {
            calendar.nextDate(
                after: now,
                matching: DateComponents(hour: hour, minute: minute, weekday: weekday),
                matchingPolicy: .nextTime
            ) ?? now
        }

        let quota = [
            QuotaEntry(
                provider: "claude", window: "5h", status: "available",
                usedPct: 32, remainingPct: 68,
                resetsAt: seconds(nextClock(hour: 14, minute: 30)),
                source: "statusline", capturedAt: ago(180), reason: nil
            ),
            QuotaEntry(
                provider: "claude", window: "7d", status: "available",
                usedPct: 59, remainingPct: 41,
                resetsAt: seconds(nextWeekday(6, hour: 9, minute: 0)),
                source: "statusline", capturedAt: ago(180), reason: nil
            ),
            QuotaEntry(
                provider: "codex", window: "week", status: "available",
                usedPct: 83, remainingPct: 17,
                resetsAt: seconds(nextWeekday(2, hour: 0, minute: 0)),
                source: "rollout_experimental", capturedAt: ago(300), reason: nil
            ),
        ]

        return Snapshot(
            sessions: sessions,
            attention: attention,
            commands: commands,
            quota: quota,
            stats: Snapshot.empty.stats
        )
    }
}
