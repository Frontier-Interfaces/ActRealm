import Foundation
import Testing
@testable import ActRealmKit

struct ModelDecodingTests {
    @Test func runtimeStatusDecodesEveryH7LayerAndConditionalFeaturesStayNeutral() throws {
        let status = try JSONDecoder().decode(RuntimeStatusSnapshot.self, from: Data(#"""
        {
          "schemaVersion": 2,
          "generatedAt": 1800000000000,
          "instanceId": "019f0000-0000-7000-8000-000000000001",
          "pid": 42,
          "version": "0.1.0",
          "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          "protocolVersion": 2,
          "startedAt": 1799999990000,
          "uptimeMs": 10000,
          "api": {"status":"ready","address":"127.0.0.1:43111"},
          "websocket": {"status":"ready","connections":1},
          "hook": {"status":"ready","name":"bridge.sock","private":true,"lastEventAt":1799999999000},
          "sessions": {"active":1,"total":2},
          "snapshot": {"revision":55,"revisionSource":"runtime:sqlite_event_count","lastEventAt":1799999999000,"freshness":"live"},
          "attention": {"pending":0,"waiters":0},
          "storage": {"status":"ready","eventCount":55,"schemaVersion":34,"expectedSchemaVersion":34,"integrity":"ok","checkedAt":1800000000000},
          "collectors": {
            "review": {"status":"ready","source":"runtime:review_baseline_queue","gitCheck":"on_demand","pendingBaselines":0,"inProgress":false,"consecutiveFailures":0,"lastSuccessfulAt":1800000000000},
            "token": {"status":"ready","source":"runtime:canonical_session_ledger","dataQuality":"verified","inProgress":false,"historyComplete":true,"consecutiveFailures":0,"lastSuccessfulAt":1800000000000}
          },
          "companion": {"status":"ready","protocolVersion":2,"registrations":1,"scopes":["snapshot.read"]},
          "conditional": {
            "claudeCowork":{"status":"unsupported","countsAsFault":false,"reason":"no_verified_event_source"}
          },
          "restart": {"count":0,"lastResult":"not_restarted"}
        }
        """#.utf8))

        #expect(status.isSupported)
        #expect(status.storage.integrity == "ok")
        #expect(status.collectors?.review.gitCheck == "on_demand")
        #expect(status.collectors?.token.historyComplete == true)
        #expect(status.companion?.scopes == ["snapshot.read"])
        #expect(status.conditional?.claudeCowork.status == "unsupported")
    }

    @Test func historyResponseDecodesOnlyBoundedTaskSummaries() throws {
        let response = try JSONDecoder().decode(RuntimeTaskHistoryResponse.self, from: Data(#"""
        {
          "schemaVersion": 1,
          "generatedAt": 1800000000000,
          "tasks": [{
            "id": "session-1",
            "provider": "codex",
            "project": "ActRealm-Cloud",
            "title": "History center",
            "model": "gpt-test",
            "status": "completed",
            "startedAt": 1799999000000,
            "lastEventAt": 1800000000000,
            "completedAt": 1799999999000,
            "archivedAt": 1800000000000,
            "archiveReason": "history_archived",
            "branch": "feature/history",
            "validationState": "passed",
            "checkpointCount": 2,
            "securityEventCount": 1,
            "jumpCapability": "exact_conversation",
            "jumpLabel": "Open exact conversation"
          }]
        }
        """#.utf8))
        #expect(response.schemaVersion == RuntimeTaskHistoryResponse.supportedSchemaVersion)
        #expect(response.tasks.first?.project == "ActRealm-Cloud")
        #expect(response.tasks.first?.checkpointCount == 2)
        #expect(response.tasks.first?.branch == "feature/history")
    }

    @Test func reviewDiffDecodesLocalFileListAndBoundedPatch() throws {
        let response = try JSONDecoder().decode(RuntimeReviewDiffResponse.self, from: Data(#"""
        {
          "schemaVersion": 1,
          "sessionId": "session-1",
          "base": "0123456789ab",
          "attribution": "bounded_window",
          "files": [
            {"path":"Sources/App.swift","state":"tracked"},
            {"path":"Notes.txt","state":"untracked"}
          ],
          "selected": {
            "path": "Sources/App.swift",
            "patch": "@@ -1 +1 @@",
            "truncated": false
          },
          "limitation": null
        }
        """#.utf8))
        #expect(response.files.count == 2)
        #expect(response.files.first?.id == "Sources/App.swift")
        #expect(response.selected?.truncated == false)
        #expect(response.attribution == "bounded_window")
    }

    @Test func reviewSnapshotDecodesOnlyBoundedLocalEvidence() throws {
        let review = try JSONDecoder().decode(RuntimeTaskReviewSnapshot.self, from: Data(#"""
        {
          "schemaVersion": 1,
          "sessionId": "session-1",
          "provider": "codex",
          "projectLabel": "actrealm",
          "generatedAt": 1800000000000,
          "turnStartedAt": 1799999900000,
          "turnEndedAt": null,
          "outcome": {
            "state": "running",
            "source": "runtime:terminal_reducer",
            "verification": "not_applicable",
            "observedAt": 1800000000000
          },
          "repository": {
            "state": "available",
            "baselineState": "available",
            "baselineCapturedAt": 1799999900100,
            "baselineHead": "fedcba987654",
            "commitCount": 1,
            "branch": "feature/review",
            "head": "0123456789ab",
            "worktreeKind": "linked",
            "dirty": true,
            "changedFiles": 3,
            "stagedFiles": 1,
            "unstagedFiles": 1,
            "untrackedFiles": 1,
            "insertions": 24,
            "deletions": 8,
            "binaryFiles": 0,
            "attribution": "current_worktree_unattributed",
            "attributionReason": "turn_baseline_unavailable"
          },
          "validations": [{
            "id": "event-1",
            "kind": "test",
            "state": "unverifiable",
            "source": "runtime:structured_tool_lifecycle",
            "toolName": "Bash",
            "observedAt": 1800000000000
          }],
          "lastMeaningfulAction": {
            "kind": "tool.completed",
            "state": "completed",
            "toolName": "Bash",
            "observedAt": 1800000000000
          },
          "limitations": ["turn_baseline_unavailable"]
        }
        """#.utf8))
        #expect(review.schemaVersion == 1)
        #expect(review.repository.branch == "feature/review")
        #expect(review.repository.changedFiles == 3)
        #expect(review.repository.baselineState == "available")
        #expect(review.repository.commitCount == 1)
        #expect(review.validations.first?.state == "unverifiable")
        #expect(review.limitations == ["turn_baseline_unavailable"])
    }

    @Test func checkpointDecodesOnlySafeSummaryAndPreflightBlockers() throws {
        let checkpoint = try JSONDecoder().decode(RuntimeTaskCheckpoint.self, from: Data(#"""
        {
          "schemaVersion": 1,
          "id": "019f0000-0000-7000-8000-000000000001",
          "sessionId": "session-1",
          "turnId": "turn-1",
          "label": "Before restore",
          "kind": "git_snapshot",
          "provider": "codex",
          "providerResumeCapability": "exact_conversation",
          "createdAt": 1800000000000,
          "repository": {
            "state": "available",
            "branch": "feature/checkpoint",
            "head": "0123456789ab",
            "worktreeKind": "linked",
            "dirty": true,
            "changedFiles": 2,
            "stagedFiles": 0,
            "unstagedFiles": 1,
            "untrackedFiles": 1,
            "gitSnapshot": true,
            "gitObject": "abcdef012345"
          },
          "validations": [],
          "validationIsHistorical": true,
          "limitations": ["untracked_not_captured"]
        }
        """#.utf8))
        #expect(checkpoint.repository.gitSnapshot)
        #expect(checkpoint.repository.changedFiles == 2)
        #expect(checkpoint.validationIsHistorical)
        #expect(checkpoint.limitations == ["untracked_not_captured"])

        let preflight = try JSONDecoder().decode(RuntimeCheckpointPreflight.self, from: Data(#"""
        {
          "schemaVersion": 1,
          "checkpointId": "019f0000-0000-7000-8000-000000000001",
          "action": "restore_code",
          "allowed": false,
          "blockers": ["working_tree_dirty"],
          "warnings": ["validation_is_historical"],
          "currentBranch": "feature/checkpoint",
          "currentHead": "0123456789ab0123456789ab0123456789ab0123",
          "currentDirty": true,
          "currentChangedFiles": 1,
          "validationIsHistorical": true
        }
        """#.utf8))
        #expect(!preflight.allowed)
        #expect(preflight.blockers == ["working_tree_dirty"])
    }

    @Test func factMetadataFailsClosedForFutureSchemasAndPrivateSourceShapes() throws {
        let future = try JSONDecoder().decode(RuntimeFactMetadata.self, from: Data(#"""
        {
          "schemaVersion": 9,
          "sourceKind": "authoritative",
          "sourceId": "runtime:future",
          "capturedAt": 1000,
          "freshness": "live",
          "verification": "verified",
          "absenceReason": null,
          "capability": "direct"
        }
        """#.utf8))
        #expect(future.sourceKind == .unavailable)
        #expect(future.sourceId == nil)
        #expect(future.capability == .unavailable)

        let privateSource = try JSONDecoder().decode(RuntimeFactMetadata.self, from: Data(#"""
        {
          "schemaVersion": 1,
          "sourceKind": "observed",
          "sourceId": "/Users/alice/private",
          "capturedAt": 1000,
          "freshness": "live",
          "verification": "verified",
          "absenceReason": null,
          "capability": "observe_only"
        }
        """#.utf8))
        #expect(privateSource.sourceId == nil)
    }

    @Test func completionAutoHideDeadlineDecodesIntoTheOutboxProjection() throws {
        let data = Data(#"""
        {
          "id": "completion-1",
          "sessionId": "session-1",
          "provider": "codex",
          "project": "actrealm",
          "requestId": null,
          "kind": "completion",
          "title": "Task completed; waiting for confirmation",
          "detail": null,
          "state": "open",
          "risk": "unknown",
          "riskNotes": [],
          "commandPreview": null,
          "expiresAt": null,
          "autoHideAt": 1801000,
          "reminderAcknowledgedAt": 1200000,
          "reminderResolution": "reminder_acknowledged",
          "createdAt": 1000,
          "resolution": null
        }
        """#.utf8)
        let attention = try JSONDecoder().decode(AttentionRecord.self, from: data)
        #expect(attention.autoHideAt == 1_801_000)
        #expect(attention.reminderAcknowledgedAt == 1_200_000)
        #expect(attention.reminderResolution == "reminder_acknowledged")
        let entry = OutboxEntry(attention: attention, sessionTitle: "Task")
        #expect(entry.autoHideAt == ZhFormat.date(fromMillis: 1_801_000))
    }

    @Test func decodesSnapshotShapeFromServer() throws {
        let json = """
        {
          "sessions": [
            {
              "id": "sess-1",
              "provider": "claude",
              "providerSessionId": "abc123",
              "project": "actrealm",
              "title": "Refactor server auth",
              "providerTitle": "Fix macOS parity",
              "providerTitleSource": "session_meta",
              "model": "claude-sonnet-5",
              "execState": "tool_running",
              "approvalOwner": null,
              "activity": "Editing files",
              "activitySince": 1737000000000,
              "planDone": 2,
              "planTotal": 5,
              "planSteps": [
                {
                  "id": "turn-1:0",
                  "text": "Refactor connector",
                  "detail": "Use official lifecycle events",
                  "status": "in_progress",
                  "source": "codex_turn_plan"
                }
              ],
              "inputTokens": 48000,
              "outputTokens": 1300,
              "turnStartedAt": 1737000000000,
              "tokenTotal": 49300,
              "contextWindowTokens": 258400,
              "lastTurnTokens": 2300,
              "contextUsedTokens": 49300,
              "contextUsedPercent": 19,
              "estimatedCostUsdMicros": 123456,
              "currentTool": "Edit",
              "currentToolCategory": "file_edit",
              "currentTarget": "LanesSection.swift",
              "activeSubagents": 2,
              "subagents": [
                {
                  "id": "child-1",
                  "agentType": "gpt-5.6-sol",
                  "status": "running",
                  "source": "codex_app_server"
                }
              ],
              "environment": "Terminal · zsh",
              "jumpCapability": "terminal",
              "jumpLabel": "返回 Terminal",
              "controlCapability": "external_hook",
              "recoveryState": "observing",
              "canManage": false,
              "usageCapturedAt": 1737000090000,
              "facts": {
                "schemaVersion": 1,
                "plan": {
                  "schemaVersion": 1,
                  "sourceKind": "authoritative",
                  "sourceId": "connector:turn/plan/updated",
                  "capturedAt": 1737000100000,
                  "freshness": "live",
                  "verification": "verified",
                  "absenceReason": null,
                  "capability": "observe_only"
                },
                "activity": {
                  "schemaVersion": 1,
                  "sourceKind": "observed",
                  "sourceId": "provider:tool_lifecycle",
                  "capturedAt": 1737000100000,
                  "freshness": "live",
                  "verification": "verified",
                  "absenceReason": null,
                  "capability": "observe_only"
                },
                "currentTarget": {
                  "schemaVersion": 1,
                  "sourceKind": "observed",
                  "sourceId": "hook:tool_input/allowlisted_basename",
                  "capturedAt": 1737000100000,
                  "freshness": "live",
                  "verification": "verified",
                  "absenceReason": null,
                  "capability": "observe_only"
                },
                "completion": {
                  "schemaVersion": 1,
                  "sourceKind": "unavailable",
                  "sourceId": null,
                  "capturedAt": 1737000100000,
                  "freshness": "live",
                  "verification": "not_applicable",
                  "absenceReason": "task_not_completed",
                  "capability": "observe_only"
                },
                "control": {
                  "schemaVersion": 1,
                  "sourceKind": "observed",
                  "sourceId": "hook:session_observation",
                  "capturedAt": 1737000100000,
                  "freshness": "live",
                  "verification": "partial",
                  "absenceReason": null,
                  "capability": "observe_only"
                }
              },
              "lastEventAt": 1737000100000
            }
          ],
          "attention": [
            {
              "id": "att-1",
              "sessionId": "sess-1",
              "provider": "claude",
              "project": "actrealm",
              "requestId": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
              "kind": "approval",
              "title": "Run rm -rf build/",
              "detail": "Requested by session sess-1",
              "state": "open",
              "risk": "high",
              "riskNotes": ["destructive"],
              "primaryCategory": "shell.remove",
              "riskCodes": ["attention.risk.high_impact", "attention.risk.irreversible"],
              "commandPreview": "rm -rf build/",
              "expiresAt": null,
              "createdAt": 1737000100000,
              "resolution": null
            },
            {
              "id": "att-question",
              "sessionId": "sess-1",
              "provider": "claude",
              "project": "actrealm",
              "requestId": "00000000-0000-0000-0000-000000000002",
              "kind": "question",
              "title": "选择交付方式",
              "detail": null,
              "state": "open",
              "risk": "low",
              "riskNotes": [],
              "commandPreview": null,
              "expiresAt": 1737003700000,
              "createdAt": 1737000100000,
              "resolution": null,
              "interaction": {
                "requestId": "00000000-0000-0000-0000-000000000002",
                "kind": "claude_question",
                "provider": "claude",
                "title": "选择交付方式",
                "message": "请选择一项",
                "expiresAt": 1737003700000,
                "supportsNative": true,
                "questions": [{
                  "id": "delivery",
                  "label": "交付",
                  "prompt": "如何交付？",
                  "inputType": "choice",
                  "multiSelect": false,
                  "isSecret": false,
                  "required": true,
                  "allowsOther": true,
                  "options": [{"label":"PR","description":"创建草稿 PR"}]
                }]
              }
            }
          ],
          "commands": [],
          "quota": [
            {
              "provider": "claude",
              "window": "5h",
              "status": "available",
              "usedPct": 42.5,
              "remainingPct": 57.5,
              "resetsAt": 1737003600,
              "resetSource": "statusline",
              "resetCapturedAt": 1737000000000,
              "source": "statusline",
              "windowMinutes": 300,
              "limitId": "five_hour",
              "limitName": "5 小时",
              "planType": "Max",
              "capturedAt": 1737000000000
            },
            {
              "provider": "codex",
              "window": "week",
              "status": "unavailable",
              "source": "unsupported_version",
              "reason": "unsupported desktop version"
            }
          ],
          "tokenUsage": {
            "today": 1200000,
            "month": 8900000,
            "total": 42000000,
            "recordedFrom": 1736900000000,
            "capturedAt": 1737000000000
          },
          "stats": {
            "eventCount": 12,
            "metrics": {
              "activeDays": 3,
              "approvalRequests": 4,
              "widgetApprovals": 2,
              "widgetDenials": 1,
              "passThroughManual": 0,
              "passThroughTimeout": 1,
              "decisionResponseMsTotal": 5000,
              "decisionResponseCount": 3,
              "bannersShown": 6,
              "sessionsObserved": 2,
              "appOpened": 5,
              "todayWidgetDecisions": 2
            }
          },
          "capabilities": {
            "codexConnector": {
              "enabled": true,
              "status": "connected",
              "managedThreads": 1,
              "managedApprovals": true,
              "serverUserAgent": "codex_cli_rs/0.144.6",
              "lastNotificationMethod": "turn/plan/updated",
              "lastNotificationAt": 1737000100000,
              "lastPlanSkipReason": "missing_plan_array",
              "lastPlanFieldKeys": ["threadId", "futureField"],
              "error": null
            },
            "providerMatrix": {
              "schemaVersion": 1,
              "providers": {
                "claude": {
                  "plan": {
                    "status": "supported",
                    "source": "hook:TaskCreated/TaskCompleted"
                  },
                  "subagents": {
                    "status": "supported",
                    "source": "hook:SubagentStart/SubagentStop"
                  },
                  "approvals": {
                    "status": "supported",
                    "source": "hook:PermissionRequest"
                  },
                  "transcriptSlice": {
                    "status": "supported",
                    "source": "hook:transcript_path/claude_jsonl_v1"
                  }
                },
                "codex": {
                  "subagents": {
                    "status": "unknown"
                  }
                }
              }
            }
          }
        }
        """

        let snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8))

        #expect(snapshot.sessions.count == 1)
        #expect(snapshot.sessions[0].providerSessionId == "abc123")
        #expect(snapshot.sessions[0].providerTitle == "Fix macOS parity")
        #expect(snapshot.sessions[0].planDone == 2)
        #expect(snapshot.sessions[0].planSteps.first?.text == "Refactor connector")
        #expect(snapshot.sessions[0].planSteps.first?.status == "in_progress")
        #expect(snapshot.sessions[0].totalTokens == 49_300)
        #expect(snapshot.sessions[0].contextWindowTokens == 258_400)
        #expect(snapshot.sessions[0].contextUsedPercent == 19)
        #expect(snapshot.sessions[0].estimatedCostUsdMicros == 123_456)
        #expect(snapshot.sessions[0].currentToolCategory == "file_edit")
        #expect(snapshot.sessions[0].currentTarget == "LanesSection.swift")
        #expect(snapshot.sessions[0].subagents.first?.agentType == "gpt-5.6-sol")
        #expect(snapshot.sessions[0].controlCapability == "external_hook")
        #expect(snapshot.sessions[0].facts?.plan.sourceKind == .authoritative)
        #expect(snapshot.sessions[0].facts?.plan.verification == .verified)
        #expect(snapshot.sessions[0].facts?.currentTarget.sourceId == "hook:tool_input/allowlisted_basename")
        #expect(snapshot.sessions[0].facts?.completion.absenceReason == .taskNotCompleted)
        #expect(snapshot.sessions[0].facts?.control.capability == .observeOnly)

        #expect(snapshot.attention.count == 2)
        #expect(snapshot.attention[0].kind == "approval")
        #expect(snapshot.attention[0].requestId?.uuidString.lowercased() == "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d")
        #expect(snapshot.attention[0].riskNotes == ["destructive"])
        #expect(snapshot.attention[0].primaryCategory == "shell.remove")
        #expect(snapshot.attention[0].riskCodes == [
            "attention.risk.high_impact", "attention.risk.irreversible"
        ])
        #expect(snapshot.attention[1].primaryCategory == nil)
        #expect(snapshot.attention[1].riskCodes == nil)
        #expect(snapshot.attention[1].interaction?.questions.first?.options.first?.label == "PR")

        #expect(snapshot.quota.count == 2)
        #expect(snapshot.quota[0].usedPct == 42.5)
        #expect(snapshot.quota[0].limitName == "5 小时")
        #expect(snapshot.quota[0].resetSource == "statusline")
        #expect(snapshot.quota[0].resetCapturedAt == 1_737_000_000_000)
        #expect(snapshot.quota[1].usedPct == nil)
        #expect(snapshot.quota[1].status == "unavailable")
        #expect(snapshot.tokenUsage.today == 1_200_000)
        #expect(snapshot.tokenUsage.month == 8_900_000)
        #expect(snapshot.tokenUsage.total == 42_000_000)
        #expect(snapshot.tokenUsage.recordedFrom == 1_736_900_000_000)

        #expect(snapshot.stats.eventCount == 12)
        #expect(snapshot.stats.metrics.approvalRequests == 4)
        #expect(snapshot.capabilities?.codexConnector?.managedThreads == 1)
        #expect(snapshot.capabilities?.codexConnector?.managedApprovals == true)
        #expect(snapshot.capabilities?.codexConnector?.serverUserAgent == "codex_cli_rs/0.144.6")
        #expect(
            snapshot.capabilities?.codexConnector?.lastNotificationMethod
                == "turn/plan/updated"
        )
        #expect(snapshot.capabilities?.codexConnector?.lastNotificationAt == 1_737_000_100_000)
        #expect(
            snapshot.capabilities?.codexConnector?.lastPlanFieldKeys
                == ["threadId", "futureField"]
        )
        #expect(snapshot.providerCapability(for: .claude, feature: .plan).status == .supported)
        #expect(
            snapshot.providerCapability(for: .claude, feature: .plan).source
                == "hook:TaskCreated/TaskCompleted"
        )
        #expect(snapshot.providerCapability(for: .claude, feature: .transcriptSlice).status == .supported)
        #expect(
            snapshot.providerCapability(
                for: .claude,
                feature: .transcriptSlice
            ).source == "hook:transcript_path/claude_jsonl_v1"
        )
        #expect(snapshot.providerCapability(for: .codex, feature: .subagents).status == .unknown)
        #expect(
            snapshot.providerCapability(for: .custom("future"), feature: .plan).status == .unknown
        )
    }

    @Test func decodesSnapshotWebSocketEnvelope() throws {
        let json = """
        {"type":"snapshot","snapshot":\(emptySnapshotJSON)}
        """
        let envelope = try JSONDecoder().decode(SnapshotEnvelope.self, from: Data(json.utf8))
        #expect(envelope.type == "snapshot")
        #expect(envelope.snapshot.sessions.isEmpty)
    }

    @Test func providerCapabilitiesFailClosedForLegacyAndFutureContracts() throws {
        let legacy = try JSONDecoder().decode(Snapshot.self, from: Data(emptySnapshotJSON.utf8))
        #expect(legacy.providerCapability(for: .claude, feature: .plan).status == .unknown)
        #expect(!legacy.providerCapability(for: .claude, feature: .plan).status.canClaimSupport)

        let future = """
        {
          "sessions": [],
          "attention": [],
          "commands": [],
          "quota": [],
          "stats": {
            "eventCount": 0,
            "metrics": {
              "activeDays": 0,
              "approvalRequests": 0,
              "widgetApprovals": 0,
              "widgetDenials": 0,
              "passThroughManual": 0,
              "passThroughTimeout": 0,
              "decisionResponseMsTotal": 0,
              "decisionResponseCount": 0,
              "bannersShown": 0,
              "sessionsObserved": 0,
              "appOpened": 0,
              "todayWidgetDecisions": 0
            }
          },
          "capabilities": {
            "providerMatrix": {
              "schemaVersion": 1,
              "providers": {
                "claude": {
                  "plan": {
                    "status": "future-status",
                    "source": "hook:FuturePlan"
                  },
                  "subagents": {
                    "status": "supported"
                  },
                  "approvals": {
                    "status": "unsupported"
                  }
                }
              }
            }
          }
        }
        """
        let snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(future.utf8))
        #expect(snapshot.providerCapability(for: .claude, feature: .plan).status == .unknown)
        #expect(snapshot.providerCapability(for: .claude, feature: .subagents).status == .unknown)
        #expect(snapshot.providerCapability(for: .claude, feature: .approvals).status == .unsupported)
        #expect(snapshot.providerCapability(for: .claude, feature: .transcriptSlice).status == .unknown)
    }

    @Test func commandRequestEncodesExpectedFieldNames() throws {
        let command = CommandRequest(
            id: UUID(uuidString: "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d")!,
            attentionId: "att-1",
            requestId: UUID(uuidString: "00000000-0000-0000-0000-000000000001"),
            action: AttentionAction.approve.rawValue
        )
        let data = try JSONEncoder().encode(command)
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["attentionId"] as? String == "att-1")
        #expect(object["action"] as? String == "approve")
        #expect(object["requestId"] != nil)
    }


    @Test func settingsEncodeTheCompleteRuntimeContract() throws {
        var settings = UISettings.defaults
        #expect(settings.taskCardFields.contains("sessionTokens"))
        #expect(settings.taskCardFields.contains("turnTokens"))
        #expect(settings.taskCardFields.contains("inputOutputTokens"))
        #expect(settings.taskCardFields.contains("cacheTokens"))
        #expect(settings.taskCardFields.contains("reasoningTokens"))
        #expect(settings.taskCardFields.contains("cost"))
        #expect(settings.taskCardFields.contains("taskFlow"))
        #expect(settings.taskCardFields.contains("workflow"))
        #expect(settings.taskCardFields.contains("currentTarget"))
        #expect(settings.displayFieldsVersion == 5)
        #expect(settings.quotaDisplayMode == .full)
        #expect(settings.tokenUsageHeatmapVisible)
        #expect(settings.tokenUsageCostVisible)
        #expect(settings.tokenUsageObservedTimeVisible)
        #expect(settings.tokenUsageExecutionTimeVisible)
        #expect(settings.tokenUsageTaskProjectVisible)
        #expect(settings.tokenUsageBurnRateVisible)
        #expect(settings.tokenUsageAnomalyVisible)
        #expect(!settings.tokenThresholdNotificationsEnabled)
        #expect(settings.tokenThresholdTokensPerMinute == 250_000)
        #expect(settings.completionTaskHideMode == .afterConfirmation)
        #expect(settings.completionAutoHideMinutes == 30)
        #expect(QuotaDisplayMode.singleLine.rawValue == "compact")
        settings.notificationRules.question = .ignore
        settings.providerMuted.codex = true
        settings.retentionDays = 180
        settings.displayProfile = "developer"
        settings.taskCardFields = ["task", "providerTurnId"]
        settings.quotaDisplayMode = .compact
        settings.tokenUsageDisplayMode = .compact
        settings.tokenUsageHeatmapVisible = false
        settings.tokenUsageCostVisible = false
        settings.tokenUsageObservedTimeVisible = false
        settings.tokenUsageExecutionTimeVisible = false
        settings.tokenUsageTaskProjectVisible = false
        settings.tokenUsageBurnRateVisible = false
        settings.tokenUsageAnomalyVisible = false
        settings.tokenThresholdNotificationsEnabled = true
        settings.tokenThresholdTokensPerMinute = 500_000
        settings.completionTaskHideMode = .afterDelay
        settings.completionAutoHideMinutes = 15

        let data = try JSONEncoder().encode(settings)
        let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        let rules = try #require(object["notificationRules"] as? [String: Any])
        let muted = try #require(object["providerMuted"] as? [String: Any])
        #expect(rules["question"] as? String == "ignore")
        #expect(muted["codex"] as? Bool == true)
        #expect(object["retentionDays"] as? Int == 180)
        #expect(object["taskCardFields"] as? [String] == ["task", "providerTurnId"])
        #expect(object["quotaDisplayMode"] as? String == "twoLine")
        #expect(object["tokenUsageDisplayMode"] as? String == "compact")
        #expect(object["tokenUsageHeatmapVisible"] as? Bool == false)
        #expect(object["tokenUsageCostVisible"] as? Bool == false)
        #expect(object["tokenUsageObservedTimeVisible"] as? Bool == false)
        #expect(object["tokenUsageExecutionTimeVisible"] as? Bool == false)
        #expect(object["tokenUsageTaskProjectVisible"] as? Bool == false)
        #expect(object["tokenUsageBurnRateVisible"] as? Bool == false)
        #expect(object["tokenUsageAnomalyVisible"] as? Bool == false)
        #expect(object["tokenThresholdNotificationsEnabled"] as? Bool == true)
        #expect(object["tokenThresholdTokensPerMinute"] as? Int == 500_000)
        #expect(object["completionTaskHideMode"] as? String == "afterDelay")
        #expect(object["completionAutoHideMinutes"] as? Int == 15)
    }

    @Test func settingsResponseDecodesTheRuntimeTokenVisibilityContract() throws {
        let response = try JSONDecoder().decode(
            SettingsResponse.self,
            from: Data(#"""
            {
              "settings": {
                "notificationRules": {
                  "approval": "list",
                  "question": "list",
                  "error": "list",
                  "completion": "list"
                },
                "soundEnabled": true,
                "providerMuted": { "claude": false, "codex": false },
                "codexEnhancedActivity": true,
                "retentionDays": 90,
                "displayProfile": "detailed",
                "taskCardFields": ["task", "taskFlow", "workflow"],
                "displayFieldsVersion": 5,
                "quotaDisplayMode": "standard",
                "tokenUsageDisplayMode": "standard",
                "tokenUsageComponentsVisible": true,
                "tokenUsageHeatmapVisible": false,
                "tokenUsageCostVisible": false,
                "tokenUsageObservedTimeVisible": false,
                "tokenUsageExecutionTimeVisible": false,
                "tokenUsageUnitStyle": "automatic",
                "tokenUsageTaskProjectVisible": false,
                "tokenUsageBurnRateVisible": false,
                "tokenUsageAnomalyVisible": false,
                "tokenThresholdNotificationsEnabled": true,
                "tokenThresholdTokensPerMinute": 100000,
                "completionTaskHideMode": "afterDelay",
                "completionAutoHideMinutes": 15
              },
              "displayCatalog": [{
                "id": "task",
                "label": "Task",
                "level": "concise",
                "placement": "headline",
                "description": "Title"
              }],
              "claudeQuotaBridge": {
                "status": "installed",
                "configPath": null,
                "helperPath": null,
                "customConflict": false
              },
              "backups": { "count": 2, "totalBytes": 128 }
            }
            """#.utf8)
        )

        #expect(!response.settings.tokenUsageHeatmapVisible)
        #expect(!response.settings.tokenUsageCostVisible)
        #expect(!response.settings.tokenUsageObservedTimeVisible)
        #expect(!response.settings.tokenUsageExecutionTimeVisible)
        #expect(!response.settings.tokenUsageTaskProjectVisible)
        #expect(!response.settings.tokenUsageBurnRateVisible)
        #expect(!response.settings.tokenUsageAnomalyVisible)
        #expect(response.settings.tokenThresholdNotificationsEnabled)
        #expect(response.settings.tokenThresholdTokensPerMinute == 100_000)
        #expect(response.displayCatalog.map(\.id) == ["task"])
        #expect(response.backups.count == 2)
    }

    @Test func legacySettingsDefaultQuotaDisplayModeToFull() throws {
        let legacy = Data(#"""
        {
          "notificationRules": {
            "approval": "banner",
            "question": "list",
            "error": "banner",
            "completion": "list"
          },
          "soundEnabled": true,
          "providerMuted": { "claude": false, "codex": false },
          "codexEnhancedActivity": true,
          "retentionDays": 90,
          "displayProfile": "detailed",
          "taskCardFields": ["project", "task"],
          "displayFieldsVersion": 3
        }
        """#.utf8)

        let settings = try JSONDecoder().decode(UISettings.self, from: legacy)
        #expect(settings.quotaDisplayMode == .full)
        #expect(settings.tokenUsageDisplayMode == .full)
        #expect(settings.tokenUsageHeatmapVisible)
        #expect(settings.tokenUsageCostVisible)
        #expect(settings.tokenUsageObservedTimeVisible)
        #expect(settings.tokenUsageExecutionTimeVisible)
        #expect(settings.tokenUsageTaskProjectVisible)
        #expect(settings.tokenUsageBurnRateVisible)
        #expect(settings.tokenUsageAnomalyVisible)
        #expect(!settings.tokenThresholdNotificationsEnabled)
        #expect(settings.tokenThresholdTokensPerMinute == 250_000)
        #expect(settings.completionTaskHideMode == .afterConfirmation)
        #expect(settings.completionAutoHideMinutes == 30)
    }

    @Test func previousCompactQuotaModeRemainsTheSingleLineMode() throws {
        let legacy = Data(#"""
        {
          "notificationRules": {
            "approval": "banner",
            "question": "list",
            "error": "banner",
            "completion": "list"
          },
          "soundEnabled": true,
          "providerMuted": { "claude": false, "codex": false },
          "codexEnhancedActivity": true,
          "retentionDays": 90,
          "displayProfile": "detailed",
          "taskCardFields": ["project", "task"],
          "displayFieldsVersion": 3,
          "quotaDisplayMode": "compact"
        }
        """#.utf8)

        let settings = try JSONDecoder().decode(UISettings.self, from: legacy)
        #expect(settings.quotaDisplayMode == .singleLine)
    }

    @Test func conciseTaskCardPresetKeepsCoreFieldsAndExpandedActivity() {
        #expect(TaskCardDisplayPresets.concise.count == 9)
        #expect(TaskCardDisplayPresets.concise == [
            "project", "task", "model", "activity", "plan", "sessionTokens", "context",
            "taskFlow", "workflow",
        ])
    }

    @Test func tokenBreakdownAndEstimatedCostFieldsEncodeIndependently() throws {
        var settings = UISettings.defaults
        settings.displayProfile = "custom"
        for field in [
            "sessionTokens", "turnTokens", "inputOutputTokens", "cacheTokens",
            "reasoningTokens", "cost", "taskFlow", "workflow",
            "currentTarget",
        ] {
            settings.taskCardFields = [field]
            let data = try JSONEncoder().encode(settings)
            let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
            #expect(object["taskCardFields"] as? [String] == [field])
        }
    }

    @Test func tokenDecisionDecodesAttributionBurnRateAndThresholdWithoutPaths() throws {
        let decision = try JSONDecoder().decode(
            TokenUsageDecisionSummary.self,
            from: Data(#"""
            {
              "schemaVersion": 2,
              "source": "runtime:canonical_session_ledger",
              "generatedAt": 10000,
              "freshness": "live",
              "capturedAt": 9000,
              "totalTokens": 1000,
              "attributedTokens": 750,
              "unattributedTokens": 250,
              "attributionCoverageBasisPoints": 7500,
              "projectAttributedTokens": 950,
              "projectUnattributedTokens": 50,
              "projectAttributionCoverageBasisPoints": 9500,
              "taskAttributedTokens": 750,
              "taskUnattributedTokens": 250,
              "taskAttributionCoverageBasisPoints": 7500,
              "projectTotals": [{
                "project": "ActRealm-Cloud",
                "total": 950,
                "taskCount": 1,
                "sessionCount": 3,
                "capturedAt": 9000
              }],
              "taskTotals": [{
                "sessionId": "session",
                "provider": "codex",
                "project": "ActRealm-Cloud",
                "title": "Token decision",
                "model": "gpt-test",
                "total": 750,
                "capturedAt": 9000
              }],
              "burnRates": [{
                "sessionId": "session",
                "turnId": "turn",
                "provider": "codex",
                "project": "ActRealm-Cloud",
                "title": "Token decision",
                "windowSeconds": 60,
                "tokenDelta": 60000,
                "tokensPerMinute": 60000,
                "sampleCount": 3,
                "baselineTokensPerMinute": 20000,
                "ratioBasisPoints": 30000,
                "state": "elevated",
                "capturedAt": 9000,
                "thresholdExceeded": true
              }],
              "thresholdTokensPerMinute": 50000
            }
            """#.utf8)
        )
        #expect(decision.attributionCoverageBasisPoints == 7_500)
        #expect(decision.projectAttributionCoverageBasisPoints == 9_500)
        #expect(decision.projectUnattributedTokens == 50)
        #expect(decision.taskAttributionCoverageBasisPoints == 7_500)
        #expect(decision.projectTotals.first?.project == "ActRealm-Cloud")
        #expect(decision.projectTotals.first?.sessionCount == 3)
        #expect(decision.burnRates.first?.state == "elevated")
        #expect(decision.burnRates.first?.thresholdExceeded == true)
        #expect(decision.thresholdTokensPerMinute == 50_000)
    }

    private var emptySnapshotJSON: String {
        """
        {"sessions":[],"attention":[],"commands":[],"quota":[],"stats":{"eventCount":0,"metrics":{"activeDays":0,"approvalRequests":0,"widgetApprovals":0,"widgetDenials":0,"passThroughManual":0,"passThroughTimeout":0,"decisionResponseMsTotal":0,"decisionResponseCount":0,"bannersShown":0,"sessionsObserved":0,"appOpened":0,"todayWidgetDecisions":0}}}
        """
    }
}
