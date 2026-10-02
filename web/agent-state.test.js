const test = require("node:test");
const assert = require("node:assert/strict");
const state = require("./agent-state.js");

test("task priority follows the user-attention contract", () => {
  const sessions = [
    { id: "run", execState: "tool_running", turnStartedAt: 30 },
    { id: "question", execState: "response_finished" },
    { id: "native", execState: "response_finished" },
    { id: "approval", execState: "response_finished" },
    { id: "error", execState: "failed" },
  ];
  const attention = [
    { sessionId: "question", kind: "question", state: "open", createdAt: 1 },
    { sessionId: "native", kind: "native_approval", state: "open", createdAt: 2 },
    { sessionId: "approval", kind: "approval", state: "open", createdAt: 3 },
  ];
  sessions.sort((a, b) => state.compareSessions(a, b, attention));
  assert.deepEqual(sessions.map((item) => item.id), ["error", "approval", "native", "question", "run"]);
});

test("acknowledged completion does not become an Outbox blocker", () => {
  assert.equal(state.isAcknowledgedCompletion({
    kind: "completion",
    reminderAcknowledgedAt: 100,
  }), true);
});


test("task card deletion follows event, turn and question watermarks without changing requests", () => {
  const session = { id: "s", execState: "tool_running", lastEventAt: 100, turnStartedAt: 90 };
  const pending = [{ sessionId: "s", kind: "approval", state: "open", createdAt: 100 }];
  const removedAt = state.deletionWatermark(session, 200, pending);
  assert.equal(state.isTaskDeleted(session, removedAt, pending), true);
  assert.equal(state.isTaskDeleted({ ...session, lastEventAt: 201 }, removedAt, pending), false);
  assert.equal(state.isTaskDeleted({ ...session, turnStartedAt: 201 }, removedAt, pending), false);
  assert.equal(state.isTaskDeleted(session, removedAt, [{ sessionId: "s", kind: "question", createdAt: 201 }]), false);
  assert.equal(pending[0].state, "open");
  assert.equal(session.execState, "tool_running");
  assert.equal(state.isTaskDeleted(session, NaN, pending), false);
});
