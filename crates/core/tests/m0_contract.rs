use actrealm_core::{
    permission_deadline_ms, permission_directive, ApprovalOwner, BridgeRequest, BridgeResponse,
    Decision, EventKind, ExecState, PendingDecision, PendingDecisionState, Provider, ReplyAction,
    SessionProjection, CLAUDE_PERMISSION_DEADLINE_MS, CODEX_PERMISSION_DEADLINE_MS,
    PERMISSION_COMMIT_DELAY_MS,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn provider_permission_directives_are_exact_minimal_json() {
    for provider in [Provider::Claude, Provider::Codex] {
        assert_eq!(
            permission_directive(provider, Decision::Allow),
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow" }
                }
            }))
        );
        assert_eq!(
            permission_directive(provider, Decision::Deny),
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {
                        "behavior": "deny",
                        "message": "User denied via ActRealm"
                    }
                }
            }))
        );
    }
}

#[test]
fn v1_permission_timing_is_provider_aligned() {
    assert_eq!(CLAUDE_PERMISSION_DEADLINE_MS, 86_400_000);
    assert_eq!(CODEX_PERMISSION_DEADLINE_MS, 3_600_000);
    assert_eq!(permission_deadline_ms(Provider::Claude), Some(86_400_000));
    assert_eq!(permission_deadline_ms(Provider::Codex), Some(3_600_000));
    assert_eq!(permission_deadline_ms(Provider::Gemini), None);
    assert_eq!(PERMISSION_COMMIT_DELAY_MS, 3_000);
}

#[test]
fn undo_before_commit_writes_no_decision() {
    let request_id = Uuid::now_v7();
    let mut pending = PendingDecision::new(request_id, 10_000, 70_000);

    pending.propose(Decision::Allow, 12_000).unwrap();
    assert_eq!(
        pending.state(),
        PendingDecisionState::Committing(Decision::Allow)
    );
    assert_eq!(pending.take_due(14_999), None);
    pending.undo(14_999).unwrap();
    assert_eq!(pending.state(), PendingDecisionState::Open);
    assert_eq!(pending.take_due(15_000), None);
}

#[test]
fn pass_through_wins_before_a_decision_is_sent() {
    let request_id = Uuid::now_v7();
    let mut pending = PendingDecision::new(request_id, 10_000, 70_000);

    pending.propose(Decision::Deny, 12_000).unwrap();
    pending.pass_through("user", 13_000).unwrap();

    assert_eq!(pending.state(), PendingDecisionState::PassedThrough);
    assert_eq!(pending.take_due(20_000), None);
}

#[test]
fn decision_sent_requires_later_provider_evidence_to_confirm() {
    let mut session = SessionProjection::default();
    session.apply(EventKind::SessionStarted, 1);
    session.apply(EventKind::PromptSubmitted, 2);
    session.apply(EventKind::ToolStarted, 3);
    session.apply(EventKind::ToolFinished, 4);
    session.apply(EventKind::PermissionRequested, 5);

    assert_eq!(session.exec_state(), ExecState::AwaitingApproval);
    assert_eq!(session.approval_owner(), Some(ApprovalOwner::Widget));

    session.mark_decision_sent(Decision::Allow, 6);
    assert_eq!(session.exec_state(), ExecState::AwaitingApproval);
    assert!(!session.decision_confirmed());

    session.apply(EventKind::ToolFinished, 7);
    assert_eq!(session.exec_state(), ExecState::Thinking);
    assert!(session.decision_confirmed());

    session.apply(EventKind::Stopped, 8);
    assert_eq!(session.exec_state(), ExecState::ResponseFinished);
}

#[test]
fn another_codex_hook_denial_cannot_make_our_allow_look_confirmed() {
    let mut session = SessionProjection::default();
    session.apply(EventKind::PromptSubmitted, 1);
    session.apply(EventKind::PermissionRequested, 2);
    session.mark_decision_sent(Decision::Allow, 3);

    // Codex emits Stop even when another matching hook vetoes the action.
    // Stop alone is turn completion, not evidence that our allow executed.
    session.apply(EventKind::Stopped, 4);

    assert_eq!(session.exec_state(), ExecState::ResponseFinished);
    assert!(!session.decision_confirmed());
}

#[test]
fn provider_denial_confirms_a_sent_deny_and_clears_widget_ownership() {
    let mut session = SessionProjection::default();
    session.apply(EventKind::PromptSubmitted, 1);
    session.apply(EventKind::PermissionRequested, 2);
    session.mark_decision_sent(Decision::Deny, 3);

    session.apply(EventKind::PermissionDenied, 4);

    assert_eq!(session.exec_state(), ExecState::Thinking);
    assert_eq!(session.approval_owner(), None);
    assert!(session.decision_confirmed());
}

#[test]
fn late_tool_events_do_not_revive_a_finished_turn() {
    let mut session = SessionProjection::default();
    session.apply(EventKind::PromptSubmitted, 1);
    session.apply(EventKind::Stopped, 2);
    session.apply(EventKind::ToolStarted, 1);
    assert_eq!(session.exec_state(), ExecState::ResponseFinished);
}

#[test]
fn permission_envelope_has_request_id_and_hard_deadline() {
    let request = BridgeRequest::from_hook_at(
        Provider::Codex,
        json!({
            "hook_event_name": "PermissionRequest",
            "session_id": "session",
            "turn_id": "turn"
        }),
        50_000,
    );

    assert!(request.needs_reply);
    assert!(request.request_id.is_some());
    assert_eq!(request.provider_session_id.as_deref(), Some("session"));
    assert_eq!(request.provider_turn_id.as_deref(), Some("turn"));
    assert_eq!(request.deadline_at, Some(3_650_000));

    let stop = BridgeRequest::from_hook_at(
        Provider::Codex,
        json!({ "hook_event_name": "Stop", "session_id": "session" }),
        50_000,
    );
    assert!(!stop.needs_reply);
    assert_eq!(stop.request_id, None);
    assert_eq!(stop.deadline_at, None);
}

#[test]
fn runtime_reply_frame_represents_pass_through_explicitly() {
    let request_id = Uuid::now_v7();
    let response = BridgeResponse::pass_through(request_id, "user");
    assert_eq!(response.request_id, request_id);
    assert_eq!(response.action, ReplyAction::PassThrough);
    assert_eq!(response.reason.as_deref(), Some("user"));
    assert_eq!(response.decision(), None);

    assert_eq!(
        BridgeResponse::decided(request_id, Decision::Deny).decision(),
        Some(Decision::Deny)
    );
}
