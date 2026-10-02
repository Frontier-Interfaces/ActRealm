import Foundation
import Testing
@testable import ActRealmKit

private func makeAttention(
    id: String,
    kind: String,
    createdAt: UInt64,
    state: String = "open",
    risk: String = "low",
    primaryCategory: String? = nil,
    commandPreview: String? = nil,
    title: String = "允许 Bash？",
    titleMessage: RuntimeMessage? = nil,
    sessionId: String = "s",
    autoHideAt: UInt64? = nil,
    reminderAcknowledgedAt: UInt64? = nil
) -> AttentionRecord {
    AttentionRecord(
        id: id, sessionId: sessionId, provider: "codex", project: "proj",
        requestId: kind == "approval" ? UUID() : nil, kind: kind, title: title,
        titleMessage: titleMessage,
        detail: nil, state: state, risk: risk, riskNotes: [],
        primaryCategory: primaryCategory,
        commandPreview: commandPreview,
        expiresAt: nil, autoHideAt: autoHideAt,
        reminderAcknowledgedAt: reminderAcknowledgedAt,
        createdAt: createdAt, resolution: nil
    )
}

private func makeSession(
    id: String,
    provider: String = "claude",
    execState: String = "idle",
    title: String? = nil,
    currentTool: String? = nil,
    recoveryState: String? = nil,
    lastEventAt: UInt64 = 1000
) -> SessionRecord {
    SessionRecord(
        id: id, provider: provider, providerSessionId: id, project: "proj",
        title: title ?? "任务 \(id)", model: nil, execState: execState, approvalOwner: nil,
        activity: nil, activitySince: nil, planDone: nil, planTotal: nil,
        currentTool: currentTool, recoveryState: recoveryState,
        lastEventAt: lastEventAt
    )
}

private func makeSnapshot(
    sessions: [SessionRecord] = [],
    attention: [AttentionRecord] = [],
    commands: [CommandRecord] = [],
    quota: [QuotaEntry] = []
) -> Snapshot {
    Snapshot(
        sessions: sessions, attention: attention, commands: commands,
        quota: quota, stats: Snapshot.empty.stats
    )
}

@Suite struct TaskFactPresentationTests {
    @Test func directRuntimeFactIsExplicitAndDoesNotExposeRawSourceData() {
        let fact = RuntimeFactMetadata(
            sourceKind: .authoritative,
            sourceId: "runtime:live_reply_waiter",
            freshness: .live,
            verification: .verified,
            capability: .direct
        )
        #expect(TaskFactPresentation.summary(
            fact,
            language: .simplifiedChinese
        ) == "Runtime · 实时 · 已验证 · 可直接处理")
    }

    @Test func unavailableFactExplainsProviderAbsenceInEnglish() {
        let fact = RuntimeFactMetadata(
            sourceKind: .unavailable,
            freshness: .stale,
            verification: .unverified,
            absenceReason: .providerNotSupplied,
            capability: .unavailable
        )
        #expect(TaskFactPresentation.summary(
            fact,
            language: .english
        ) == "No source · Stale · Unverified · Provider did not supply it")
    }
}

@Suite struct OutboxOrderingTests {
    @Test func stableSelectionSurvivesPriorityReordering() {
        let first = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "approval", kind: "approval", createdAt: 100),
            makeAttention(id: "question", kind: "question", createdAt: 200),
        ])).openOutbox
        #expect(OutboxSelection.resolvedID(currentID: "question", entries: first) == "question")

        let reordered = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "error", kind: "error", createdAt: 300),
            makeAttention(id: "question", kind: "question", createdAt: 200),
            makeAttention(id: "approval", kind: "approval", createdAt: 100),
        ])).openOutbox
        #expect(OutboxSelection.resolvedID(currentID: "question", entries: reordered) == "question")
    }

    @Test func resolvedSelectionFallsBackToHighestPriorityOpenItem() {
        let entries = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "approval", kind: "approval", createdAt: 100),
            makeAttention(id: "question", kind: "question", createdAt: 200),
        ])).openOutbox
        #expect(OutboxSelection.resolvedID(currentID: "gone", entries: entries) == "approval")
        #expect(OutboxSelection.resolvedID(currentID: "gone", entries: []) == nil)
    }

    @Test func newHigherPriorityArrivalReplacesAVisibleCompletion() {
        let previous = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "done", kind: "completion", createdAt: 100)
        ])).openOutbox
        let next = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "done", kind: "completion", createdAt: 100),
            makeAttention(id: "approval", kind: "approval", createdAt: 200),
        ])).openOutbox

        #expect(OutboxSelection.resolvedID(
            currentID: "done",
            previousEntries: previous,
            entries: next
        ) == "approval")
    }

    @Test func lowerPriorityArrivalDoesNotInterruptAnApprovalBeingReviewed() {
        let previous = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "approval", kind: "approval", createdAt: 100)
        ])).openOutbox
        let next = DerivedState.derive(from: makeSnapshot(attention: [
            makeAttention(id: "approval", kind: "approval", createdAt: 100),
            makeAttention(id: "done", kind: "completion", createdAt: 200),
        ])).openOutbox

        #expect(OutboxSelection.resolvedID(
            currentID: "approval",
            previousEntries: previous,
            entries: next
        ) == "approval")
    }

    @Test func errorsBeatApprovalsBeatQuestionsBeatCompletions() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(id: "done", kind: "completion", createdAt: 100),
            makeAttention(id: "ask", kind: "question", createdAt: 200),
            makeAttention(id: "approve", kind: "approval", createdAt: 300),
            makeAttention(id: "boom", kind: "error", createdAt: 400),
        ])
        let derived = DerivedState.derive(from: snapshot)
        #expect(derived.outbox.map(\.id) == ["boom", "approve", "ask", "done"])
    }

    @Test func oldestWaitsFirstWithinSameKind() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(id: "newer", kind: "approval", createdAt: 2000),
            makeAttention(id: "older", kind: "approval", createdAt: 1000),
        ])
        let derived = DerivedState.derive(from: snapshot)
        #expect(derived.outbox.map(\.id) == ["older", "newer"])
    }

    @Test func resolvedItemsAreHidden() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(id: "gone", kind: "approval", createdAt: 100, state: "confirmed"),
            makeAttention(id: "live", kind: "approval", createdAt: 100),
        ])
        let derived = DerivedState.derive(from: snapshot)
        #expect(derived.outbox.map(\.id) == ["live"])
    }

    @Test func acknowledgedAutomaticCompletionLeavesDoneTaskButClosesOutboxReminder() throws {
        let derived = DerivedState.derive(from: makeSnapshot(
            sessions: [makeSession(id: "s", execState: "response_finished")],
            attention: [makeAttention(
                id: "done",
                kind: "completion",
                createdAt: 100,
                autoHideAt: 2_000,
                reminderAcknowledgedAt: 1_500
            )]
        ))
        #expect(derived.openOutbox.isEmpty)
        let task = try #require(derived.agentTasks.first)
        #expect(task.status == .done)
        #expect(task.hasVisibleAttention)
        #expect(task.openOutboxCount == 0)
    }

    @Test func snoozedItemsStayOutOfOutboxUntilRuntimeReopensThem() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(id: "later", kind: "completion", createdAt: 100, state: "snoozed")
        ])
        let derived = DerivedState.derive(from: snapshot)
        #expect(derived.outbox.map(\.id) == ["later"])
        #expect(derived.openOutbox.isEmpty)
    }

    @Test func toolNameParsedFromRuntimeTitle() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(id: "a", kind: "approval", createdAt: 1, title: "允许 Bash？")
        ])
        let entry = DerivedState.derive(from: snapshot).outbox[0]
        #expect(entry.toolName == "Bash")
        #expect(entry.actionTitle == "Codex 请求运行 Bash，等待批准")
    }

    @Test func riskWarningRemainsIndependentOfApprovalCapabilities() {
        #expect(RiskLevel(record: "high").needsVerification)
        #expect(RiskLevel(record: "unknown").needsVerification)
        #expect(RiskLevel(record: "med").needsVerification)
        #expect(!RiskLevel(record: "low").needsVerification)
    }

    @Test func providerNativeApprovalIsNotPresentedAsDirectlyAnswerable() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(id: "native", kind: "native_approval", createdAt: 100)
        ])
        let entry = DerivedState.derive(from: snapshot).openOutbox[0]
        #expect(entry.kind == .nativeApproval)
        #expect(entry.actionTitle == "允许 Bash？")
    }

    @Test func completionHeadlineUsesRuntimeCodeInBothLanguages() {
        let snapshot = makeSnapshot(attention: [
            makeAttention(
                id: "done",
                kind: "completion",
                createdAt: 100,
                title: "Task completed; waiting for confirmation"
            )
        ])
        let entry = DerivedState.derive(from: snapshot).openOutbox[0]

        #expect(entry.actionTitleMessage.code == "attention.completion.title")
        #expect(entry.localizedActionTitle(language: .english)
            == "Task completed; waiting for confirmation")
        #expect(entry.localizedActionTitle(language: .simplifiedChinese)
            == "任务已完成，等待确认")
    }

    @Test func taskTitlesRemainVerbatimWhenTheyResembleLegacyStatusText() throws {
        let task = try #require(DerivedState.derive(from: makeSnapshot(
            sessions: [makeSession(
                id: "user-title",
                title: "正在运行 我的发布计划"
            )]
        )).agentTasks.first)

        #expect(task.localizedTitle(language: .english) == "正在运行 我的发布计划")
    }
}

@Suite struct RuntimeControlAvailabilityTests {
    @MainActor
    @Test func disconnectedRuntimeCannotMutateAttention() {
        let suite = "RuntimeControlAvailabilityTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let model = AppModel(defaults: defaults, demo: false)
        #expect(!model.canControlRuntime)
    }
}

@Suite struct AcknowledgementSemanticsTests {
    @Test func completionAndErrorConfirmationsNameTheAcceptedOutcome() throws {
        let completion = try #require(DerivedState.derive(from: makeSnapshot(
            sessions: [makeSession(id: "s")],
            attention: [makeAttention(
                id: "done",
                kind: "completion",
                createdAt: 1,
                sessionId: "s"
            )]
        )).openOutbox.first)
        #expect(
            AppModel.acknowledgementToast(for: completion)
                == "已确认任务完成：任务 s"
        )

        let automaticCompletion = try #require(DerivedState.derive(from: makeSnapshot(
            sessions: [makeSession(id: "auto")],
            attention: [makeAttention(
                id: "auto-done",
                kind: "completion",
                createdAt: 1,
                sessionId: "auto",
                autoHideAt: 2_000
            )]
        )).openOutbox.first)
        #expect(
            AppModel.acknowledgementToast(for: automaticCompletion)
                == "已关闭完成提醒：任务 auto"
        )

        let error = try #require(DerivedState.derive(from: makeSnapshot(
            sessions: [makeSession(id: "s")],
            attention: [makeAttention(
                id: "error",
                kind: "error",
                createdAt: 1,
                sessionId: "s"
            )]
        )).openOutbox.first)
        #expect(
            AppModel.acknowledgementToast(for: error)
                == "已标记问题解决：任务 s"
        )
    }
}

@Suite struct BackgroundSnapshotProjectionTests {
    @Test func sessionOnlyUpdatesWaitUntilTheWorkspaceIsVisible() {
        let next = makeSnapshot(sessions: [makeSession(
            id: "live",
            provider: "codex",
            execState: "thinking",
            lastEventAt: 2
        )])

        #expect(!SnapshotProjectionPolicy.shouldApply(
            previous: .empty,
            next: next,
            workspaceVisible: false
        ))
        #expect(SnapshotProjectionPolicy.shouldApply(
            previous: .empty,
            next: next,
            workspaceVisible: true
        ))
    }

    @Test func attentionAndCommandsStillProjectInTheBackground() {
        let nextAttention = makeSnapshot(attention: [
            makeAttention(id: "approval", kind: "approval", createdAt: 2)
        ])
        #expect(SnapshotProjectionPolicy.shouldApply(
            previous: .empty,
            next: nextAttention,
            workspaceVisible: false
        ))

        let nextCommand = makeSnapshot(commands: [
            CommandRecord(
                id: UUID(), attentionId: "approval", requestId: nil,
                action: "approve", state: "committing", createdAt: 2
            )
        ])
        #expect(SnapshotProjectionPolicy.shouldApply(
            previous: .empty,
            next: nextCommand,
            workspaceVisible: false
        ))
    }
}

@Suite struct RuntimeStatusMessageTests {
    @Test func delayedControlRecoveryReplacesTheTemporaryWarning() {
        #expect(RuntimeStatusMessagePolicy.reconciled(
            RuntimeStatusMessagePolicy.waitingForControl,
            status: .listening
        ) == RuntimeStatusMessagePolicy.reconnected)
    }

    @Test func aLaterDisconnectCannotKeepAStaleSuccessClaim() {
        #expect(RuntimeStatusMessagePolicy.reconciled(
            RuntimeStatusMessagePolicy.reconnected,
            status: .absent("连接已断开")
        ) == nil)
    }

    @Test func anUnrelatedFailureMessageIsNotHidden() {
        #expect(RuntimeStatusMessagePolicy.reconciled(
            "重启失败：端口被占用",
            status: .absent("端口被占用")
        ) == "重启失败：端口被占用")
    }
}

@Suite struct QuotaSlotTests {
    @Test func manualRefreshTracksNewestClaudeCaptureOnly() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "claude", window: "5h", status: "available",
                usedPct: 10, remainingPct: 90, resetsAt: nil, source: "oauth",
                capturedAt: 1_000, reason: nil
            ),
            QuotaEntry(
                provider: "claude", window: "7d", status: "available",
                usedPct: 20, remainingPct: 80, resetsAt: nil, source: "oauth",
                capturedAt: 2_000, reason: nil
            ),
            QuotaEntry(
                provider: "codex", window: "7d", status: "available",
                usedPct: 30, remainingPct: 70, resetsAt: nil, source: "session",
                capturedAt: 9_000, reason: nil
            ),
        ])
        #expect(AppModel.latestClaudeQuotaCapture(in: snapshot) == 2_000)
    }

    @Test func missingQuotaDoesNotInventFixedWindows() {
        let derived = DerivedState.derive(from: makeSnapshot())
        #expect(derived.quotaSlots.isEmpty)
    }

    @Test func availableEntryMapsRemainingPct() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "claude", window: "5h", status: "available",
                usedPct: 32, remainingPct: 68, resetsAt: 1_784_193_000,
                resetSource: "statusline", resetCapturedAt: 900_000, source: "oauth_usage",
                capturedAt: 1_000_000, reason: nil
            )
        ])
        let slot = DerivedState.derive(from: snapshot).quotaSlots[0]
        guard case .available(let pct, let resetsAt, _) = slot.availability else {
            Issue.record("expected available")
            return
        }
        #expect(pct == 68)
        #expect(resetsAt?.timeIntervalSince1970 == 1_784_193_000)
        #expect(slot.source == "oauth_usage")
        #expect(slot.resetSource == "statusline")
        #expect(slot.resetCapturedAt?.timeIntervalSince1970 == 900)
        #expect(!slot.isTight)
    }

    @Test func staleQuotaPreservesItsLastValueWithoutClaimingCurrentAvailability() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "claude", window: "5h", status: "stale",
                usedPct: 32, remainingPct: 68, resetsAt: nil, source: "oauth_usage",
                capturedAt: 1_000_000, reason: "temporary refresh failure"
            )
        ])
        let slot = DerivedState.derive(from: snapshot).quotaSlots[0]
        guard case .stale(let pct, _, _) = slot.availability else {
            Issue.record("expected an explicitly stale last-known value")
            return
        }
        #expect(pct == 68)
    }

    @Test func below20PercentIsTight() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "codex", window: "week", status: "available",
                usedPct: 83, remainingPct: 17, resetsAt: nil, source: "rollout",
                capturedAt: nil, reason: nil
            )
        ])
        let slot = DerivedState.derive(from: snapshot).quotaSlots[0]
        #expect(slot.isTight)
    }

    @Test func keepsEveryRuntimeWindowAndMetadataInServerOrder() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "codex", window: "primary", status: "available",
                usedPct: 10, remainingPct: 90, resetsAt: nil, source: "oauth_usage",
                windowMinutes: 300, limitId: "codex", limitName: "主窗口",
                planType: "Plus", capturedAt: 1_000, reason: nil
            ),
            QuotaEntry(
                provider: "codex", window: "fable", status: "available",
                usedPct: 20, remainingPct: 80, resetsAt: nil, source: "oauth_usage",
                windowMinutes: 1_440, limitId: "fable", limitName: "Fable",
                planType: "Plus", capturedAt: 1_000, reason: nil
            ),
        ])
        let slots = DerivedState.derive(from: snapshot).quotaSlots
        #expect(slots.map(\.title) == ["主窗口", "Fable"])
        #expect(slots.map(\.planType) == ["Plus", "Plus"])
        #expect(slots.map(\.windowMinutes) == [300, 1_440])
    }

    @Test func proSparkQuotaIsNamedSeparatelyAndPlanLabelsAreReadable() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "codex", window: "10080m", status: "available",
                usedPct: 1, remainingPct: 99, resetsAt: nil, source: "codex_app_server",
                windowMinutes: 10_080, limitId: "codex", planType: "pro",
                capturedAt: 1_000, reason: nil
            ),
            QuotaEntry(
                provider: "codex", window: "10080m", status: "available",
                usedPct: 0, remainingPct: 100, resetsAt: nil, source: "codex_app_server",
                windowMinutes: 10_080, limitId: "codex_bengalfox", quotaKind: "spark",
                planType: "pro",
                capturedAt: 1_000, reason: nil
            ),
        ])

        let slots = DerivedState.derive(from: snapshot).quotaSlots
        #expect(slots.map(\.providerDisplayName) == ["Codex", "Codex Spark"])
        #expect(slots.map(\.displayPlanType) == ["pro", "pro"])
        #expect(slots.map(\.isSpark) == [false, true])
    }

    @Test func sparkQuotaIsHiddenUnlessCodexReportsAProPlan() {
        for planType in ["plus", "business", nil] as [String?] {
            let snapshot = makeSnapshot(quota: [
                QuotaEntry(
                    provider: "codex", window: "10080m", status: "available",
                    usedPct: 10, remainingPct: 90, resetsAt: nil,
                    source: "codex_app_server", windowMinutes: 10_080,
                    limitId: "codex", planType: planType, capturedAt: 1_000, reason: nil
                ),
                QuotaEntry(
                    provider: "codex", window: "10080m", status: "available",
                    usedPct: 20, remainingPct: 80, resetsAt: nil,
                    source: "codex_app_server", windowMinutes: 10_080,
                    limitId: "codex_bengalfox", quotaKind: "spark", planType: planType,
                    capturedAt: 1_000, reason: nil
                ),
            ])

            let slots = DerivedState.derive(from: snapshot).quotaSlots
            #expect(slots.count == 1)
            #expect(slots[0].providerDisplayName == "Codex")
        }
    }

    @Test func unknownPlanLabelIsPreservedInsteadOfGuessed() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "codex", window: "300m", status: "available",
                usedPct: 10, remainingPct: 90, resetsAt: nil, source: "codex_app_server",
                windowMinutes: 300, limitId: "codex", planType: "future-tier",
                capturedAt: 1_000, reason: nil
            ),
        ])

        #expect(DerivedState.derive(from: snapshot).quotaSlots[0].displayPlanType == "future-tier")
    }

    @Test func calendarMonthQuotaDoesNotRenderAsHundredsOfHours() {
        let snapshot = makeSnapshot(quota: [
            QuotaEntry(
                provider: "codex", window: "43800m", status: "available",
                usedPct: 60, remainingPct: 40, resetsAt: 1_788_219_474,
                source: "codex_app_server", windowMinutes: 43_800,
                capturedAt: 1_785_744_057_000, reason: nil
            )
        ])
        #expect(DerivedState.derive(from: snapshot).quotaSlots[0].title == "1 month")
    }
}

@Suite struct LaneTests {
    @Test func claudeAndCodexLanesAlwaysExist() {
        let derived = DerivedState.derive(from: makeSnapshot())
        #expect(Set(derived.lanes.map(\.provider)) == Set([.claude, .codex]))
    }

    @Test func futureProviderGetsItsOwnLaneWithoutNativeClientChanges() {
        let snapshot = makeSnapshot(sessions: [
            makeSession(id: "cursor-1", provider: "cursor-agent", execState: "thinking")
        ])
        let derived = DerivedState.derive(from: snapshot)
        let provider = ProviderKind(record: "cursor-agent")
        #expect(provider == .custom("cursor-agent"))
        #expect(derived.lanes.first(where: { $0.provider == provider })?.tasks.count == 1)
    }

    @Test func newestTaskIsAlwaysLeftmostInsideLane() {
        let snapshot = makeSnapshot(
            sessions: [
                makeSession(id: "idle", execState: "idle", lastEventAt: 3000),
                makeSession(id: "run", execState: "tool_running", lastEventAt: 2000),
                makeSession(id: "wait", execState: "awaiting_approval", lastEventAt: 1000),
            ]
        )
        let lane = DerivedState.derive(from: snapshot).lanes.first { $0.provider == .claude }!
        #expect(lane.tasks.map(\.id) == ["idle", "run", "wait"])
        #expect(lane.pulse == .waiting)
    }

    @Test func execStatesMapToBadges() {
        func status(_ exec: String) -> LaneTaskStatus {
            LaneTask(session: makeSession(id: "x", execState: exec), openAttention: []).status
        }
        #expect(status("awaiting_approval") == .waiting)
        #expect(status("tool_running") == .running)
        #expect(status("thinking") == .running)
        #expect(status("compacting") == .running)
        #expect(status("failed") == .failed)
        #expect(status("response_finished") == .done)
        #expect(status("idle") == .idle)
    }

    @Test func agentTasksAreNewestFirstAcrossProviders() {
        let snapshot = makeSnapshot(sessions: [
            makeSession(id: "claude-old", provider: "claude", lastEventAt: 1_000),
            makeSession(id: "codex-new", provider: "codex", lastEventAt: 3_000),
            makeSession(id: "claude-mid", provider: "claude", lastEventAt: 2_000),
        ])
        #expect(DerivedState.derive(from: snapshot).agentTasks.map(\.id) == [
            "codex-new", "claude-mid", "claude-old"
        ])
    }

    @Test func openQuestionMarksTaskWaiting() {
        let task = LaneTask(
            session: makeSession(id: "x", execState: "idle"),
            openAttention: [makeAttention(id: "q", kind: "question", createdAt: 5)]
        )
        #expect(task.status == .waiting)
        #expect(task.openOutboxCount == 1)
    }

    @Test func completionWaitingForConfirmationMarksFinishedTaskWaiting() {
        let task = LaneTask(
            session: makeSession(id: "x", execState: "response_finished"),
            openAttention: [makeAttention(id: "done", kind: "completion", createdAt: 5)]
        )
        #expect(task.status == .waiting)
        #expect(task.primaryAttentionKind == .completion)
    }

    @Test func snoozedAttentionKeepsSessionVisibleWithoutBlockingIt() {
        let snoozed = makeAttention(
            id: "later", kind: "question", createdAt: 5, state: "snoozed", sessionId: "x"
        )
        let snapshot = makeSnapshot(
            sessions: [makeSession(id: "x", execState: "idle")],
            attention: [snoozed]
        )
        let task = DerivedState.derive(from: snapshot).agentTasks[0]
        #expect(task.status == .idle)
        #expect(task.openOutboxCount == 0)
        #expect(task.hasVisibleAttention)
    }
}

@Suite struct ExecutionPresentationTests {
    @Test func runningTaskAndRecoveryStatusNeverContradict() {
        let task = LaneTask(
            session: makeSession(
                id: "running",
                execState: "tool_running",
                recoveryState: "ended"
            ),
            openAttention: []
        )

        #expect(task.status == .idle)
        #expect(task.executionIsUnconfirmed)
        #expect(task.recoveryPresentation != .ended)
    }

    @Test func missingToolNameIsOmittedInsteadOfRenderedAsUnknown() {
        let task = LaneTask(
            session: makeSession(id: "running", execState: "tool_running"),
            openAttention: []
        )

        #expect(task.activityLabel == "运行中")
        #expect(task.detailRows.contains(where: { $0.value == "Unknown" }) == false)
        #expect(task.detailRows.contains(where: { $0.field == "tool" }) == false)
    }
}

@Suite struct PendingDecisionTests {
    @Test func pendingCommitIsUndoable() {
        let now = Date()
        let createdAt = UInt64(now.timeIntervalSince1970 * 1000) - 1000
        let snapshot = makeSnapshot(
            attention: [makeAttention(id: "att", kind: "approval", createdAt: createdAt, state: "committing")],
            commands: [
                CommandRecord(
                    id: UUID(), attentionId: "att", requestId: UUID(),
                    action: "approve", state: "pending_commit", createdAt: createdAt
                )
            ]
        )
        let pending = DerivedState.derive(from: snapshot, now: now).pendingDecision
        guard case .undoable(let deadline)? = pending?.phase else {
            Issue.record("expected undoable phase")
            return
        }
        #expect(pending?.attentionID == "att")
        #expect(abs(deadline.timeIntervalSince(ZhFormat.date(fromMillis: createdAt).addingTimeInterval(3))) < 0.001)
        #expect(pending?.summary.hasPrefix("将允许") == true)
    }

    @Test func oldConfirmedCommandsProduceNothing() {
        let now = Date()
        let createdAt = UInt64((now.timeIntervalSince1970 - 3600) * 1000)
        let snapshot = makeSnapshot(commands: [
            CommandRecord(
                id: UUID(), attentionId: "att", requestId: nil,
                action: "approve", state: "confirmed", createdAt: createdAt
            )
        ])
        #expect(DerivedState.derive(from: snapshot, now: now).pendingDecision == nil)
    }
}

@Suite struct FormattingTests {
    @Test func waitDurations() {
        #expect(ZhFormat.waitDuration(362) == "6 分 02 秒")
        #expect(ZhFormat.waitDuration(48) == "48 秒")
        #expect(ZhFormat.waitDuration(3900) == "1 小时 05 分")
    }

    @Test func shortAges() {
        #expect(ZhFormat.shortAge(10) == "刚刚")
        #expect(ZhFormat.shortAge(130) == "2 分")
    }

    @Test func expiryText() {
        #expect(ZhFormat.expiry(54 * 60) == "54 分钟后过期")
        #expect(ZhFormat.expiry(-5) == "已过期")
    }

    @Test func longQuotaResetUsesTheInjectedCalendarTimeZone() {
        var utc = Calendar(identifier: .gregorian)
        utc.timeZone = TimeZone(secondsFromGMT: 0)!
        let utcText = ZhFormat.resetTime(
            Date(timeIntervalSince1970: 1_788_219_474),
            now: Date(timeIntervalSince1970: 1_785_744_057),
            calendar: utc,
            language: .simplifiedChinese
        )
        #expect(utcText.contains("8月31日"))

        var shanghai = Calendar(identifier: .gregorian)
        shanghai.timeZone = TimeZone(identifier: "Asia/Shanghai")!
        let shanghaiText = ZhFormat.resetTime(
            Date(timeIntervalSince1970: 1_788_219_474),
            now: Date(timeIntervalSince1970: 1_785_744_057),
            calendar: shanghai,
            language: .simplifiedChinese
        )
        #expect(shanghaiText.contains("9月1日"))
    }

    @Test func futureQuotaResetAlwaysIncludesDateWeekdayAndTime() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Asia/Shanghai")!
        let now = calendar.date(from: DateComponents(
            year: 2026, month: 8, day: 13, hour: 16, minute: 30
        ))!
        let reset = calendar.date(from: DateComponents(
            year: 2026, month: 8, day: 20, hour: 14, minute: 59
        ))!

        #expect(ZhFormat.resetTime(
            reset,
            now: now,
            calendar: calendar,
            language: .simplifiedChinese
        ) == "8月20日 周四 14:59 重置")
        #expect(ZhFormat.resetTime(
            reset,
            now: now,
            calendar: calendar,
            language: .english
        ) == "Resets Aug 20 · Thu at 14:59")
    }

    @Test func crossYearQuotaResetIncludesTheYear() {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Asia/Shanghai")!
        let now = calendar.date(from: DateComponents(
            year: 2026, month: 12, day: 31, hour: 23
        ))!
        let reset = calendar.date(from: DateComponents(
            year: 2027, month: 1, day: 2, hour: 9, minute: 30
        ))!

        #expect(ZhFormat.resetTime(
            reset,
            now: now,
            calendar: calendar,
            language: .simplifiedChinese
        ) == "2027年1月2日 周六 09:30 重置")
        #expect(ZhFormat.resetTime(
            reset,
            now: now,
            calendar: calendar,
            language: .english
        ) == "Resets Jan 2, 2027 · Sat at 09:30")
    }
}

@Suite struct LocalTaskTruthRegressionTests {
    @Test func deadOrUnconfirmedExecutionIsExcludedFromRunningTasks() {
        for recovery in ["lost_control", "waiting_for_event", "ended"] {
            let session = makeSession(id: "s", execState: "thinking", recoveryState: recovery)
            let task = LaneTask(session: session, openAttention: [])
            #expect(task.status == .idle)
            #expect(task.executionIsUnconfirmed)
            #expect(!task.isVisibleInAgentTasks(at: Date(timeIntervalSince1970: 500)))
        }
        let live = LaneTask(session: makeSession(id: "live", execState: "tool_running", recoveryState: "observing"), openAttention: [])
        #expect(live.status == .running)
    }

    @Test func unconfirmedExecutionPreservesRealPendingRequests() {
        let task = LaneTask(session: makeSession(id: "s", execState: "thinking", recoveryState: "lost_control"), openAttention: [makeAttention(id: "approval", kind: "approval", createdAt: 1000)])
        #expect(task.status == .waiting)
        #expect(task.openOutboxCount == 1)
    }

    @Test func observedUsageIsRetainedWithoutClaimingCompleteCoverage() throws {
        let session = try JSONDecoder().decode(SessionRecord.self, from: Data(#"{"id":"s","provider":"codex","providerSessionId":"s","execState":"thinking","lastEventAt":1000,"totalTokens":12345,"lastTurnTokens":2345,"usageQuality":"partial","usageSource":"codex_rollout_during_indexing"}"#.utf8))
        let task = LaneTask(session: session, openAttention: [])
        #expect(task.totalTokens == 12345)
        #expect(task.lastTurnTokens == 2345)
        #expect(task.usageIsProvisional)
    }
}
