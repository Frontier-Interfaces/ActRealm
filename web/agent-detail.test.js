const test = require("node:test");
const assert = require("node:assert/strict");
const detail = require("./agent-detail.js");

test("current action and target keep semantic facts without guessing paths", () => {
  assert.equal(detail.currentAction({ currentTool: "Bash", currentToolCategory: "file_read" }), "文件读取 · Bash");
  assert.equal(detail.currentAction({ currentTool: "MCP node_repl.js", currentToolCategory: "code_execution" }, "en"), "Code execution · MCP node_repl.js");
  assert.equal(detail.currentTarget({ execState: "tool_running" }, "supported"), "当前工具没有文件目标");
  assert.equal(detail.currentTarget({ currentTarget: "LanesSection.swift" }, "supported"), "LanesSection.swift");
});

test("workflow pairs concurrent tools by invocation identity", () => {
  const events = [
    { eventId: "a-start", kind: "tool.started", toolName: "Bash", toolCallId: "a", occurredAt: 1 },
    { eventId: "b-start", kind: "tool.started", toolName: "Bash", toolCallId: "b", occurredAt: 2 },
    { eventId: "b-end", kind: "tool.completed", toolName: "Bash", toolCallId: "b", occurredAt: 4 },
    { eventId: "a-end", kind: "tool.completed", toolName: "Bash", toolCallId: "a", occurredAt: 7 },
  ];
  const items = detail.workflowItems(events);
  assert.equal(items.length, 2);
  assert.equal(items[0].event.eventId, "a-end");
  assert.equal(items[1].event.eventId, "b-end");
  assert.equal(items[0].startedAt, 1);
  assert.equal(items[1].startedAt, 2);
});

test("plan empty states distinguish support and turn lifecycle", () => {
  assert.equal(detail.planEmptyText("supported", true), "当前 Turn 尚未收到计划事件");
  assert.equal(detail.planEmptyText("supported", false), "当前 Turn 已结束，未提供计划");
  assert.equal(detail.planEmptyText("unsupported", true, "en"), "Provider does not support plan events");
});

test("usage and subagent facts never invent zero or completeness", () => {
  assert.equal(detail.subagentText({ activeSubagents: 0 }, "supported"), "暂无活动子 Agent");
  assert.equal(detail.subagentText({}, "unknown", "en"), "Provider subagent capability is not confirmed");
  assert.equal(detail.usageFact({ usageSource: "codex_rollout_incremental", usageQuality: "partial" }), "Codex 本机 rollout（增量） · 部分覆盖");
  assert.equal(detail.usageFact({}), "Provider 未提供用量数据 · 完整性未知");
  assert.equal(detail.quotaResetSourceLabel({ resetSource: "oauth_usage" }), "官方 OAuth");
  assert.equal(detail.quotaResetSourceLabel({}), "Provider 未提供");
});

test("fact metadata explains source, staleness, absence, and control without raw values", () => {
  assert.equal(detail.factSummary({
    schemaVersion: 1,
    sourceKind: "authoritative",
    sourceId: "runtime:live_reply_waiter",
    capturedAt: null,
    freshness: "live",
    verification: "verified",
    absenceReason: null,
    capability: "direct",
  }), "Runtime · 实时 · 已验证 · 可直接处理");
  assert.equal(detail.factSummary({
    schemaVersion: 1,
    sourceKind: "unavailable",
    sourceId: null,
    capturedAt: null,
    freshness: "stale",
    verification: "unverified",
    absenceReason: "provider_not_supplied",
    capability: "unavailable",
  }, "en"), "No source · Stale · Unverified · Provider did not supply it");
  assert.equal(detail.factSummary({schemaVersion: 9}, "en"), "No trusted fact metadata");
});
