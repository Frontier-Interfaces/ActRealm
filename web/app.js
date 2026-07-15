"use strict";

const ui = {
  runtimeState: document.querySelector("#runtime-state"),
  runtimeLabel: document.querySelector("#runtime-label"),
  offlineBanner: document.querySelector("#offline-banner"),
  attentionCount: document.querySelector("#attention-count"),
  attentionList: document.querySelector("#attention-list"),
  sessionCount: document.querySelector("#session-count"),
  sessionList: document.querySelector("#session-list"),
  quotaList: document.querySelector("#quota-list"),
  eventCount: document.querySelector("#event-count"),
  undoToast: document.querySelector("#undo-toast"),
  undoMessage: document.querySelector("#undo-message"),
  undoButton: document.querySelector("#undo-button"),
  toast: document.querySelector("#toast"),
};

let csrfToken = sessionStorage.getItem("flowAgentCsrf");
let snapshot = { sessions: [], attention: [], commands: [], quota: [], stats: {} };
let currentAttention = 0;
let socket;
let reconnectDelay = 500;
let undoCommandId;
let toastTimer;

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
}

function emptyState(icon, title, detail) {
  const root = element("div", "empty-state");
  root.append(element("div", "empty-icon", icon));
  root.append(element("h3", "", title));
  root.append(element("p", "", detail));
  return root;
}

function openItems() {
  const visibleStates = new Set(["open", "committing", "decision_sent"]);
  const weights = { error: 4, approval: 3, question: 2, completion: 1 };
  return snapshot.attention
    .filter((item) => visibleStates.has(item.state))
    .sort((a, b) => (weights[b.kind] || 0) - (weights[a.kind] || 0) || a.createdAt - b.createdAt);
}

function recentOutcome() {
  const finalStates = new Set(["confirmed", "resolved", "passed_through", "expired"]);
  return snapshot.attention
    .filter((item) => finalStates.has(item.state))
    .sort((a, b) => b.createdAt - a.createdAt)[0];
}

function outcomeSummary() {
  const item = recentOutcome();
  if (!item) return undefined;
  const command = latestCommand(item);
  const outcomeState = command?.state === "confirmed" ? "confirmed" : item.state;
  const summary = element("div", "recent-outcome");
  summary.append(element("span", "", "最近结果"));
  summary.append(element("strong", "", stateLabel(outcomeState)));
  return summary;
}

function providerName(provider) {
  return { claude: "Claude", codex: "Codex", gemini: "Gemini" }[provider] || provider || "Agent";
}

function stateLabel(state) {
  return {
    open: "等待处理",
    committing: "3 秒内可撤回",
    decision_sent: "决定已发送",
    confirmed: "已确认继续",
    resolved: "已解决",
    passed_through: "已交回终端",
    expired: "已过期，交回终端",
    snoozed: "稍后提醒",
  }[state] || state;
}

function attentionTitle(item) {
  if (item.kind === "approval") return `想运行 ${item.commandPreview || "一项工具操作"}，等你点头`;
  if (item.kind === "error") return item.title || "任务出错停下来了";
  if (item.kind === "completion") return item.title || "这一轮已经完成";
  return item.title || "Agent 有一件事需要你处理";
}

function latestCommand(item) {
  return snapshot.commands
    .filter((command) => command.attentionId === item.id)
    .sort((a, b) => b.createdAt - a.createdAt)[0];
}

function actionButton(label, className, action, item) {
  const button = element("button", `action-button ${className || ""}`.trim(), label);
  button.type = "button";
  button.addEventListener("click", () => sendAction(item, action));
  return button;
}

function renderAttention() {
  const items = openItems();
  ui.attentionCount.textContent = String(items.length);
  ui.attentionList.replaceChildren();
  if (!items.length) {
    ui.attentionList.append(emptyState("✓", "现在没有需要你处理的任务", "新的授权、问题、完成或错误会实时出现在这里。"));
    const outcome = outcomeSummary();
    if (outcome) ui.attentionList.append(outcome);
    return;
  }
  currentAttention = Math.min(currentAttention, items.length - 1);
  const item = items[currentAttention];
  const card = element("article", "attention-card");
  const kicker = element("div", "attention-kicker");
  kicker.append(element("span", "attention-kind", `${items.length} 件等你 · ${item.kind === "completion" ? "不着急" : "任务停着"}`));
  kicker.append(element("span", "attention-state", stateLabel(item.state)));
  card.append(kicker, element("h3", "", attentionTitle(item)));

  const agentLine = element("div", "agent-line");
  agentLine.append(element("span", "provider-glyph", providerName(item.provider).slice(0, 2)));
  agentLine.append(element("strong", "", providerName(item.provider)));
  if (item.project) agentLine.append(element("span", "", `· ${item.project}`));
  card.append(agentLine);

  const fact = item.detail || item.commandPreview;
  if (fact) card.append(element("div", "fact-block", fact));
  const risk = element("div", "risk-row");
  risk.append(element("span", "risk-chip", `风险标记：${item.risk || "未知"}`));
  for (const note of item.riskNotes || []) risk.append(element("span", "risk-chip", note));
  if (item.expiresAt) risk.append(element("span", "risk-chip", `截止 ${new Date(item.expiresAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`));
  card.append(risk);

  const actions = element("div", "actions");
  if (item.state === "open" && item.kind === "approval") {
    if (item.risk === "high") {
      actions.append(actionButton("去终端核对", "", "pass_through", item));
      actions.append(actionButton("不行", "deny", "deny", item));
      const approve = element("button", "action-button ghost", "批准…");
      approve.type = "button";
      approve.addEventListener("click", () => {
        if (window.confirm("这是高影响操作。确认仍要批准这一次请求？")) sendAction(item, "approve");
      });
      actions.append(approve);
    } else {
      actions.append(actionButton("批准", "", "approve", item));
      actions.append(actionButton("不行", "deny", "deny", item));
      actions.append(actionButton("去终端处理", "ghost", "pass_through", item));
    }
  } else if (item.state === "open") {
    const acknowledge = item.kind === "completion" ? "没问题，收工" : "标记已解决";
    actions.append(actionButton(acknowledge, "", "ack", item));
    actions.append(actionButton("待会提醒", "ghost", "snooze", item));
  } else if (item.state === "committing") {
    const command = latestCommand(item);
    if (command && command.state === "pending_commit") {
      const undo = element("button", "action-button ghost", "撤回决定");
      undo.type = "button";
      undo.addEventListener("click", () => undoCommand(command.id));
      actions.append(undo);
    }
  }
  card.append(actions);

  if (items.length > 1) {
    const pager = element("div", "pager");
    const previous = element("button", "", "←");
    previous.type = "button";
    previous.setAttribute("aria-label", "上一件");
    previous.addEventListener("click", () => { currentAttention = (currentAttention + items.length - 1) % items.length; renderAttention(); });
    const next = element("button", "", "→");
    next.type = "button";
    next.setAttribute("aria-label", "下一件");
    next.addEventListener("click", () => { currentAttention = (currentAttention + 1) % items.length; renderAttention(); });
    pager.append(previous, element("span", "", `第 ${currentAttention + 1}/${items.length} 件`), next);
    card.append(pager);
  }
  ui.attentionList.append(card);
  const outcome = outcomeSummary();
  if (outcome) ui.attentionList.append(outcome);
}

function sessionStatus(session) {
  const waiting = snapshot.attention.filter((item) => item.sessionId === session.id && ["open", "committing", "decision_sent"].includes(item.state)).length;
  if (waiting) return { label: `等你${waiting > 1 ? ` ×${waiting}` : ""}`, className: "waiting" };
  if (session.execState === "failed") return { label: "出错", className: "failed" };
  if (["idle", "response_finished"].includes(session.execState)) return { label: "空闲", className: "idle" };
  return { label: "在跑", className: "" };
}

function renderSessions() {
  ui.sessionList.replaceChildren();
  ui.sessionCount.textContent = `${snapshot.sessions.length} 个接入`;
  if (!snapshot.sessions.length) {
    ui.sessionList.append(emptyState("↗", "还没有 Agent 接入", "安装 Hook 后，真实会话会出现在这里。"));
    return;
  }
  for (const session of snapshot.sessions) {
    const status = sessionStatus(session);
    const row = element("article", "session-row");
    const top = element("div", "row-top");
    top.append(element("span", "provider-glyph", providerName(session.provider).slice(0, 2)));
    const copy = element("div", "row-copy");
    const title = element("div", "row-title");
    title.append(element("strong", "", session.project || providerName(session.provider)));
    title.append(element("span", `state-pill ${status.className}`.trim(), status.label));
    copy.append(title, element("div", "row-subtitle", session.activity || stateLabel(session.execState)));
    top.append(copy);
    row.append(top);
    ui.sessionList.append(row);
  }
}

function renderQuota() {
  ui.quotaList.replaceChildren();
  if (!snapshot.quota.length) {
    const unavailable = element("div", "quota-unavailable");
    unavailable.append(element("strong", "", "暂无可靠额度数据"));
    unavailable.append(element("p", "", "Flow Agent 不会用估算值冒充真实额度。采集桥接在后续里程碑接入。"));
    unavailable.append(element("div", "quota-track"));
    ui.quotaList.append(unavailable);
    return;
  }
  for (const quota of snapshot.quota) {
    const row = element("article", "quota-row");
    const title = element("div", "row-title");
    title.append(element("strong", "", `${providerName(quota.provider)} · ${quota.window || "额度"}`));
    if (typeof quota.usedPct === "number") title.append(element("span", "section-meta", `${Math.round(quota.usedPct)}%`));
    row.append(title);
    const track = element("div", "quota-track");
    const fill = element("div", "quota-fill");
    fill.style.width = `${Math.max(0, Math.min(100, Number(quota.usedPct) || 0))}%`;
    track.append(fill);
    row.append(track);
    ui.quotaList.append(row);
  }
}

function render(nextSnapshot) {
  snapshot = nextSnapshot;
  renderAttention();
  renderSessions();
  renderQuota();
  ui.eventCount.textContent = String(snapshot.stats?.eventCount || 0);
}

async function api(path, options = {}) {
  const headers = new Headers(options.headers || {});
  if (options.body && !headers.has("content-type")) headers.set("content-type", "application/json");
  if (csrfToken && options.method && options.method !== "GET") headers.set("x-flow-agent-csrf", csrfToken);
  const response = await fetch(path, { ...options, headers, credentials: "same-origin" });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(data.error?.code || `HTTP_${response.status}`);
  return data;
}

async function bootstrap() {
  const token = new URLSearchParams(location.hash.slice(1)).get("bootstrap");
  if (!token) return false;
  const response = await api("/api/v1/bootstrap", { method: "POST", body: JSON.stringify({ token }) });
  csrfToken = response.csrfToken;
  sessionStorage.setItem("flowAgentCsrf", csrfToken);
  history.replaceState(null, "", `${location.pathname}${location.search}`);
  return true;
}

async function loadSnapshot() {
  try {
    render(await api("/api/v1/snapshot"));
  } catch (error) {
    if (String(error.message) === "UNAUTHORIZED" && await bootstrap()) {
      render(await api("/api/v1/snapshot"));
      return;
    }
    throw error;
  }
}

function setConnected(connected) {
  document.body.classList.toggle("disconnected", !connected);
  ui.runtimeState.classList.toggle("online", connected);
  ui.runtimeLabel.textContent = connected ? "Live · 本地" : "正在重连";
  ui.offlineBanner.hidden = connected;
}

function connectSocket() {
  if (!csrfToken) return;
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  socket = new WebSocket(`${scheme}://${location.host}/api/v1/ws?csrf=${encodeURIComponent(csrfToken)}`);
  socket.addEventListener("open", () => { reconnectDelay = 500; setConnected(true); });
  socket.addEventListener("message", (event) => {
    try {
      const frame = JSON.parse(event.data);
      if (frame.type === "snapshot") render(frame.snapshot);
    } catch (_) {
      showToast("Runtime 返回了无法识别的消息");
    }
  });
  socket.addEventListener("close", () => {
    setConnected(false);
    window.setTimeout(connectSocket, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 10000);
  });
  socket.addEventListener("error", () => socket.close());
}

async function sendAction(item, action) {
  const id = crypto.randomUUID();
  try {
    const command = await api("/api/v1/commands", {
      method: "POST",
      body: JSON.stringify({ id, attentionId: item.id, requestId: item.requestId, action }),
    });
    if (command.state === "pending_commit") showUndo(command.id, action);
    await loadSnapshot();
  } catch (error) {
    showToast(error.message === "STALE_APPROVAL" ? "这项请求已过期，已交回原终端" : `操作失败：${error.message}`);
    await loadSnapshot().catch(() => {});
  }
}

function showUndo(commandId, action) {
  undoCommandId = commandId;
  ui.undoMessage.textContent = `${action === "approve" ? "批准" : "拒绝"} · 3 秒后提交`;
  ui.undoToast.hidden = false;
  window.setTimeout(() => {
    if (undoCommandId === commandId) {
      undoCommandId = undefined;
      ui.undoToast.hidden = true;
    }
  }, 3100);
}

async function undoCommand(commandId) {
  try {
    await api(`/api/v1/commands/${encodeURIComponent(commandId)}/undo`, { method: "POST" });
    if (undoCommandId === commandId) undoCommandId = undefined;
    ui.undoToast.hidden = true;
    await loadSnapshot();
  } catch (error) {
    showToast(error.message === "STALE_APPROVAL" ? "决定已经提交，不能再撤回" : `撤回失败：${error.message}`);
  }
}

function showToast(message) {
  window.clearTimeout(toastTimer);
  ui.toast.textContent = message;
  ui.toast.hidden = false;
  toastTimer = window.setTimeout(() => { ui.toast.hidden = true; }, 3500);
}

ui.undoButton.addEventListener("click", () => {
  if (undoCommandId) undoCommand(undoCommandId);
});

(async () => {
  setConnected(false);
  try {
    await loadSnapshot();
    connectSocket();
  } catch (error) {
    ui.attentionList.replaceChildren(emptyState("!", "无法连接本地 Runtime", "请从 flow-agent serve 输出的一次性地址打开控制面板。"));
    showToast(`连接失败：${error.message}`);
  }
})();
