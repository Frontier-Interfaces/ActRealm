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
  setupTrigger: document.querySelector("#setup-trigger"),
  setupOverlay: document.querySelector("#setup-overlay"),
  setupClose: document.querySelector("#setup-close"),
  setupProviders: document.querySelector("#setup-providers"),
  setupRefresh: document.querySelector("#setup-refresh"),
  settingsTrigger: document.querySelector("#settings-trigger"),
  settingsOverlay: document.querySelector("#settings-overlay"),
  settingsClose: document.querySelector("#settings-close"),
  notifyApproval: document.querySelector("#notify-approval"),
  notifyQuestion: document.querySelector("#notify-question"),
  notifyError: document.querySelector("#notify-error"),
  notifyCompletion: document.querySelector("#notify-completion"),
  soundEnabled: document.querySelector("#sound-enabled"),
  muteClaude: document.querySelector("#mute-claude"),
  muteCodex: document.querySelector("#mute-codex"),
  codexEnhanced: document.querySelector("#codex-enhanced"),
  retentionDays: document.querySelector("#retention-days"),
  claudeBridgeStatus: document.querySelector("#claude-bridge-status"),
  claudeBridgeAction: document.querySelector("#claude-bridge-action"),
  exportData: document.querySelector("#export-data"),
  clearData: document.querySelector("#clear-data"),
  metricsSummary: document.querySelector("#metrics-summary"),
  exportMetrics: document.querySelector("#export-metrics"),
  notificationBanner: document.querySelector("#notification-banner"),
  notificationKind: document.querySelector("#notification-kind"),
  notificationTitle: document.querySelector("#notification-title"),
  notificationView: document.querySelector("#notification-view"),
  notificationClose: document.querySelector("#notification-close"),
};

let csrfToken = sessionStorage.getItem("flowAgentCsrf");
let snapshot = { sessions: [], attention: [], commands: [], quota: [], stats: {} };
let currentAttention = 0;
let socket;
let reconnectDelay = 500;
let undoCommandId;
let toastTimer;
let setupState = { providers: [], firstRun: false };
let setupBusy = false;
let settingsState = {
  notificationRules: { approval: "banner", question: "banner", error: "banner", completion: "list" },
  soundEnabled: true,
  providerMuted: { claude: false, codex: false },
  codexEnhancedActivity: false,
  retentionDays: 90,
};
let claudeBridge = { status: "not_installed" };
let settingsBusy = false;
let notificationsPrimed = false;
let knownAttentionIds = new Set();
let notificationItemId;
let renderedEventCount = 0;
let eventUiLatencies = [];

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

function setupStatus(status) {
  return {
    not_installed: { label: "未接入", className: "muted", detail: "不会修改现有配置，点击后先备份再语义合并。" },
    cli_missing: { label: "未找到 CLI", className: "error", detail: "请先安装这个 Agent 的命令行程序。" },
    needs_trust: { label: "等待信任", className: "warning", detail: "打开 Codex，输入 /hooks，逐项检查并信任 Flow Agent。" },
    installed_unverified: { label: "等待验证", className: "warning", detail: "配置已经就绪。启动一次真实会话后才能确认接入。" },
    connected: { label: "已接入", className: "ready", detail: "已收到安装后的真实 Agent 事件，实时活动可以正常显示。" },
    needs_reinstall: { label: "配置有变化", className: "error", detail: "发现不完整或被修改的 Flow Agent 条目；不会自动覆盖。" },
    inline_conflict: { label: "配置冲突", className: "error", detail: "Codex 同时存在 inline Hook。请先保留一种同层配置形式。" },
    error: { label: "配置无法解析", className: "error", detail: "为保护你的设置，Flow Agent 已拒绝改写。请先恢复或修正配置。" },
  }[status] || { label: status, className: "muted", detail: "状态暂时无法识别。" };
}

function setupButton(label, className, handler, disabled = false) {
  const button = element("button", `setup-action ${className || ""}`.trim(), label);
  button.type = "button";
  button.disabled = disabled || setupBusy;
  button.addEventListener("click", handler);
  return button;
}

function renderSetup() {
  ui.setupProviders.replaceChildren();
  for (const provider of setupState.providers || []) {
    const status = setupStatus(provider.status);
    const card = element("article", "setup-provider");
    const heading = element("div", "setup-provider-heading");
    const identity = element("div", "setup-identity");
    identity.append(element("span", "provider-glyph", providerName(provider.provider).slice(0, 2)));
    identity.append(element("strong", "", providerName(provider.provider)));
    heading.append(identity, element("span", `setup-status ${status.className}`, status.label));
    card.append(heading, element("p", "setup-detail", status.detail));

    if (provider.provider === "codex" && provider.status === "needs_trust") {
      const steps = element("ol", "trust-steps");
      for (const step of ["打开任意 Codex 会话", "输入 /hooks", "核对命令路径后选择信任", "启动一个新会话并回到这里刷新"]) {
        steps.append(element("li", "", step));
      }
      card.append(steps);
    }
    const path = element("div", "setup-path", provider.configPath || "");
    path.title = provider.configPath || "";
    card.append(path);
    const actions = element("div", "setup-actions");
    if (provider.status === "not_installed") {
      actions.append(setupButton("安全接入", "primary", () => changeSetup(provider.provider, "install")));
    } else if (provider.status === "needs_reinstall") {
      actions.append(setupButton("检查后重新安装", "primary", () => changeSetup(provider.provider, "install")));
    } else if (provider.canRepair) {
      actions.append(setupButton("修复二进制", "primary", () => changeSetup(provider.provider, "repair")));
    } else if (["needs_trust", "installed_unverified", "connected"].includes(provider.status)) {
      actions.append(setupButton("刷新状态", "primary", loadSetup));
      actions.append(setupButton("移除接入", "ghost", () => changeSetup(provider.provider, "uninstall")));
    } else {
      actions.append(setupButton("暂不可操作", "ghost", () => {}, true));
    }
    card.append(actions);
    ui.setupProviders.append(card);
  }
  const needsAttention = (setupState.providers || []).some((provider) => provider.status !== "connected");
  ui.setupTrigger.classList.toggle("needs-attention", needsAttention);
}

function openSetup() {
  ui.setupOverlay.hidden = false;
  ui.setupClose.focus();
}

function closeSetup() {
  ui.setupOverlay.hidden = true;
  sessionStorage.setItem("flowAgentSetupSeen", "1");
  ui.setupTrigger.focus();
}

async function loadSetup() {
  try {
    setupState = await api("/api/v1/setup");
    renderSetup();
    if (setupState.firstRun && !sessionStorage.getItem("flowAgentSetupSeen")) openSetup();
  } catch (error) {
    showToast(`接入状态读取失败：${error.message}`);
  }
}

async function changeSetup(provider, action) {
  if (setupBusy) return;
  setupBusy = true;
  renderSetup();
  try {
    setupState = await api("/api/v1/setup", {
      method: "POST",
      body: JSON.stringify({ provider, action, enhancedCodexActivity: false }),
    });
    renderSetup();
    showToast(action === "uninstall" ? `${providerName(provider)} 接入已移除` : `${providerName(provider)} 配置已安全写入`);
  } catch (error) {
    showToast(`接入操作失败：${error.detail || error.message}`);
  } finally {
    setupBusy = false;
    renderSetup();
  }
}

function openSettings() {
  ui.settingsOverlay.hidden = false;
  renderMetrics();
  ui.settingsClose.focus();
  loadSettings();
}

function closeSettings() {
  ui.settingsOverlay.hidden = true;
  ui.settingsTrigger.focus();
}

function bridgeStatusCopy(status) {
  return {
    installed: "已开启 · 等待 Claude 下一次响应更新",
    not_installed: "未开启",
    helper_missing: "桥接文件缺失，可安全修复",
    custom_conflict: "检测到你的自定义 status line，不会覆盖",
    config_malformed: "Claude 配置无法解析，已停止修改",
  }[status] || "状态暂时不可用";
}

function renderSettings() {
  const rules = settingsState.notificationRules || {};
  ui.notifyApproval.value = rules.approval || "banner";
  ui.notifyQuestion.value = rules.question || "banner";
  ui.notifyError.value = rules.error || "banner";
  ui.notifyCompletion.value = rules.completion || "list";
  ui.soundEnabled.checked = Boolean(settingsState.soundEnabled);
  ui.muteClaude.checked = Boolean(settingsState.providerMuted?.claude);
  ui.muteCodex.checked = Boolean(settingsState.providerMuted?.codex);
  ui.codexEnhanced.checked = Boolean(settingsState.codexEnhancedActivity);
  ui.retentionDays.value = String(settingsState.retentionDays || 90);
  ui.claudeBridgeStatus.textContent = bridgeStatusCopy(claudeBridge.status);
  const removable = claudeBridge.status === "installed";
  const blocked = ["custom_conflict", "config_malformed"].includes(claudeBridge.status);
  ui.claudeBridgeAction.textContent = removable ? "关闭" : claudeBridge.status === "helper_missing" ? "修复" : "开启";
  ui.claudeBridgeAction.dataset.action = removable ? "uninstall" : "install";
  ui.claudeBridgeAction.disabled = settingsBusy || blocked;
}

async function loadSettings() {
  try {
    const response = await api("/api/v1/settings");
    settingsState = response.settings;
    claudeBridge = response.claudeQuotaBridge;
    renderSettings();
  } catch (error) {
    showToast(`设置读取失败：${error.message}`);
  }
}

function settingsFromForm() {
  return {
    notificationRules: {
      approval: ui.notifyApproval.value,
      question: ui.notifyQuestion.value,
      error: ui.notifyError.value,
      completion: ui.notifyCompletion.value,
    },
    soundEnabled: ui.soundEnabled.checked,
    providerMuted: { claude: ui.muteClaude.checked, codex: ui.muteCodex.checked },
    codexEnhancedActivity: ui.codexEnhanced.checked,
    retentionDays: Number(ui.retentionDays.value),
  };
}

async function saveSettings() {
  if (settingsBusy) return;
  settingsBusy = true;
  const previousCodexMode = settingsState.codexEnhancedActivity;
  try {
    const response = await api("/api/v1/settings", {
      method: "PUT",
      body: JSON.stringify(settingsFromForm()),
    });
    settingsState = response.settings;
    claudeBridge = response.claudeQuotaBridge;
    renderSettings();
    if (previousCodexMode !== settingsState.codexEnhancedActivity) {
      showToast("Codex Hook 已更新，请在 Codex 中运行 /hooks 重新检查信任");
      loadSetup();
    } else {
      showToast("设置已保存到本机");
    }
  } catch (error) {
    renderSettings();
    showToast(`设置保存失败：${error.detail || error.message}`);
  } finally {
    settingsBusy = false;
    renderSettings();
  }
}

async function changeClaudeBridge() {
  if (settingsBusy) return;
  settingsBusy = true;
  renderSettings();
  const action = ui.claudeBridgeAction.dataset.action || "install";
  try {
    const response = await api("/api/v1/quota/claude-bridge", {
      method: "POST",
      body: JSON.stringify({ action }),
    });
    settingsState = response.settings;
    claudeBridge = response.claudeQuotaBridge;
    await loadSnapshot();
    showToast(action === "install" ? "Claude 额度桥已开启，完成一次对话后会显示额度" : "Claude 额度桥已关闭");
  } catch (error) {
    showToast(`额度桥操作失败：${error.detail || error.message}`);
  } finally {
    settingsBusy = false;
    renderSettings();
  }
}

async function exportLocalData() {
  try {
    const response = await fetch("/api/v1/export", { credentials: "same-origin" });
    if (!response.ok) throw new Error(`HTTP_${response.status}`);
    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "flow-agent-export.json";
    document.body.append(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
    showToast("本地数据已导出");
  } catch (error) {
    showToast(`导出失败：${error.message}`);
  }
}

async function exportLocalMetrics() {
  try {
    const response = await fetch("/api/v1/metrics/export", { credentials: "same-origin" });
    if (!response.ok) throw new Error(`HTTP_${response.status}`);
    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "flow-agent-metrics.json";
    document.body.append(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
    showToast("仅统计数据已导出，不含会话和事件明细");
  } catch (error) {
    showToast(`统计导出失败：${error.message}`);
  }
}

async function clearLocalData() {
  const confirmation = window.prompt("这会删除本地事件、会话、额度缓存和设置。请输入 DELETE 确认：");
  if (confirmation !== "DELETE") {
    if (confirmation !== null) showToast("输入不匹配，没有删除任何数据");
    return;
  }
  try {
    await api("/api/v1/data/clear", {
      method: "POST",
      body: JSON.stringify({ confirmation }),
    });
    notificationsPrimed = false;
    knownAttentionIds = new Set();
    await loadSnapshot();
    await loadSettings();
    showToast("本地运行数据已彻底清除，Hook 接入保持不变");
  } catch (error) {
    showToast(`清除失败：${error.detail || error.message}`);
  }
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

function renderMetrics() {
  const metrics = snapshot.stats?.metrics || {};
  const requests = Number(metrics.approvalRequests || 0);
  const decisions = Number(metrics.widgetApprovals || 0) + Number(metrics.widgetDenials || 0);
  const responses = Number(metrics.decisionResponseCount || 0);
  const panelRate = requests > 0 ? `${Math.round(decisions / requests * 100)}%` : "—";
  const timeoutRate = requests > 0 ? `${Math.round(Number(metrics.passThroughTimeout || 0) / requests * 100)}%` : "—";
  const average = responses > 0 ? `${Math.round(Number(metrics.decisionResponseMsTotal || 0) / responses / 100) / 10}s` : "—";
  const uiP95 = document.body.dataset.eventUiP95Ms
    ? `${document.body.dataset.eventUiP95Ms}ms`
    : "—";
  const values = [
    [Number(metrics.activeDays || 0), "活跃天数"],
    [decisions, "面板批准 / 拒绝"],
    [panelRate, "面板处理率"],
    [timeoutRate, "超时交还率"],
    [average, "平均响应"],
    [uiP95, "页面渲染 p95"],
  ];
  ui.metricsSummary.replaceChildren();
  for (const [value, label] of values) {
    const item = element("div", "metric-pill");
    item.append(element("strong", "", value), element("span", "", label));
    ui.metricsSummary.append(item);
  }
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
    const handled = Number(snapshot.stats?.metrics?.todayWidgetDecisions || 0);
    const detail = handled > 0
      ? `今天你已通过面板处理 ${handled} 件；新的授权、问题、完成或错误会实时出现。`
      : "新的授权、问题、完成或错误会实时出现在这里。";
    ui.attentionList.append(emptyState("✓", "现在没有需要你处理的任务", detail));
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
    if (Number.isInteger(session.planDone) && Number.isInteger(session.planTotal) && session.planTotal > 0) {
      const progress = element("div", "plan-progress");
      const label = element("span", "", `计划 ${session.planDone}/${session.planTotal}`);
      const track = element("div", "plan-track");
      track.setAttribute("role", "progressbar");
      track.setAttribute("aria-valuemin", "0");
      track.setAttribute("aria-valuemax", String(session.planTotal));
      track.setAttribute("aria-valuenow", String(session.planDone));
      const fill = element("div", "plan-fill");
      fill.style.width = `${Math.max(0, Math.min(100, session.planDone / session.planTotal * 100))}%`;
      track.append(fill);
      progress.append(label, track);
      copy.append(progress);
    }
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
    unavailable.append(element("p", "", "额度来源尚未检查；Flow Agent 不会用估算值冒充真实额度。"));
    unavailable.append(element("div", "quota-track"));
    ui.quotaList.append(unavailable);
    return;
  }
  for (const quota of snapshot.quota) {
    if (quota.status !== "available" || typeof quota.usedPct !== "number" || typeof quota.remainingPct !== "number") {
      const unavailable = element("article", "quota-unavailable");
      unavailable.append(element("strong", "", `${providerName(quota.provider)} · 暂无数据`));
      unavailable.append(element("p", "", quota.reason || "额度来源没有返回可验证数据"));
      unavailable.append(element("div", "quota-track"));
      if (quota.provider === "claude") {
        const help = element("button", "quota-help", "如何开启");
        help.type = "button";
        help.addEventListener("click", openSettings);
        unavailable.append(help);
      }
      ui.quotaList.append(unavailable);
      continue;
    }
    const row = element("article", "quota-row");
    const title = element("div", "row-title");
    title.append(element("strong", "", `${providerName(quota.provider)} · ${quota.window || "额度"}`));
    title.append(element("span", "section-meta", `剩余 ${Math.round(quota.remainingPct)}%`));
    row.append(title);
    const track = element("div", "quota-track");
    const fill = element("div", "quota-fill");
    fill.style.width = `${Math.max(0, Math.min(100, quota.remainingPct))}%`;
    fill.classList.add(quota.remainingPct >= 50 ? "healthy" : quota.remainingPct >= 20 ? "warning" : "critical");
    track.append(fill);
    row.append(track);
    const meta = element("div", "quota-meta");
    if (quota.resetsAt) {
      const reset = new Date(Number(quota.resetsAt) * 1000);
      meta.append(element("span", "", `${reset.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} 重置`));
    }
    if (quota.capturedAt) {
      const minutes = Math.floor((Date.now() - Number(quota.capturedAt)) / 60000);
      meta.append(element("span", "", minutes > 0 ? `${minutes} 分钟前更新` : "刚刚更新"));
    }
    row.append(meta);
    ui.quotaList.append(row);
  }
}

function notificationRule(item) {
  return settingsState.notificationRules?.[item.kind] || "list";
}

function isProviderMuted(provider) {
  return Boolean(settingsState.providerMuted?.[provider]);
}

function playNotificationSound() {
  if (!settingsState.soundEnabled) return;
  try {
    const AudioContextType = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextType) return;
    const context = new AudioContextType();
    const oscillator = context.createOscillator();
    const gain = context.createGain();
    oscillator.frequency.value = 660;
    gain.gain.setValueAtTime(0.0001, context.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.12, context.currentTime + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, context.currentTime + 0.12);
    oscillator.connect(gain);
    gain.connect(context.destination);
    oscillator.start();
    oscillator.stop(context.currentTime + 0.13);
    oscillator.addEventListener("ended", () => context.close());
  } catch (_) {
    // Browsers may require a user gesture before audio; the banner still works.
  }
}

function showNotification(item) {
  notificationItemId = item.id;
  ui.notificationKind.textContent = `${providerName(item.provider)} · ${item.kind === "approval" ? "等待批准" : stateLabel(item.kind)}`;
  ui.notificationTitle.textContent = attentionTitle(item);
  ui.notificationBanner.hidden = false;
  playNotificationSound();
  void recordUiMetric("banner_shown");
}

function processNotifications(nextSnapshot) {
  const nextItems = (nextSnapshot.attention || []).filter((item) => ["open", "committing", "decision_sent"].includes(item.state));
  if (notificationsPrimed) {
    const item = nextItems.find((candidate) => !knownAttentionIds.has(candidate.id));
    if (item && notificationRule(item) === "banner" && !isProviderMuted(item.provider)) showNotification(item);
  }
  knownAttentionIds = new Set(nextItems.map((item) => item.id));
}

function render(nextSnapshot) {
  processNotifications(nextSnapshot);
  snapshot = nextSnapshot;
  renderAttention();
  renderSessions();
  renderQuota();
  ui.eventCount.textContent = String(snapshot.stats?.eventCount || 0);
  const eventCount = Number(snapshot.stats?.eventCount || 0);
  if (eventCount > renderedEventCount) {
    const latest = Math.max(0, ...(snapshot.sessions || []).map((session) => Number(session.lastEventAt || 0)));
    const latency = Date.now() - latest;
    if (latest > 0 && latency >= 0 && latency <= 10000) {
      eventUiLatencies.push(latency);
      eventUiLatencies = eventUiLatencies.slice(-100);
      const sorted = [...eventUiLatencies].sort((a, b) => a - b);
      document.body.dataset.eventUiP95Ms = String(sorted[Math.max(0, Math.ceil(sorted.length * 0.95) - 1)]);
    }
    renderedEventCount = eventCount;
  }
  renderMetrics();
}

async function api(path, options = {}) {
  const headers = new Headers(options.headers || {});
  if (options.body && !headers.has("content-type")) headers.set("content-type", "application/json");
  if (csrfToken && options.method && options.method !== "GET") headers.set("x-flow-agent-csrf", csrfToken);
  const response = await fetch(path, { ...options, headers, credentials: "same-origin" });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(data.error?.code || `HTTP_${response.status}`);
    error.detail = data.error?.detail;
    throw error;
  }
  return data;
}

async function recordUiMetric(event) {
  try {
    await api("/api/v1/metrics", {
      method: "POST",
      body: JSON.stringify({ event }),
    });
  } catch (_) {
    // Metrics are local evidence only and never interfere with Agent control.
  }
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
      if (frame.type === "snapshot") {
        const previousEventCount = Number(snapshot.stats?.eventCount || 0);
        render(frame.snapshot);
        if (Number(snapshot.stats?.eventCount || 0) !== previousEventCount) loadSetup();
      }
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
ui.setupTrigger.addEventListener("click", openSetup);
ui.setupClose.addEventListener("click", closeSetup);
ui.setupRefresh.addEventListener("click", loadSetup);
ui.setupOverlay.addEventListener("click", (event) => {
  if (event.target === ui.setupOverlay) closeSetup();
});
ui.settingsTrigger.addEventListener("click", openSettings);
ui.settingsClose.addEventListener("click", closeSettings);
ui.settingsOverlay.addEventListener("click", (event) => {
  if (event.target === ui.settingsOverlay) closeSettings();
});
for (const control of [
  ui.notifyApproval,
  ui.notifyQuestion,
  ui.notifyError,
  ui.notifyCompletion,
  ui.soundEnabled,
  ui.muteClaude,
  ui.muteCodex,
  ui.codexEnhanced,
  ui.retentionDays,
]) {
  control.addEventListener("change", saveSettings);
}
ui.claudeBridgeAction.addEventListener("click", changeClaudeBridge);
ui.exportData.addEventListener("click", exportLocalData);
ui.exportMetrics.addEventListener("click", exportLocalMetrics);
ui.clearData.addEventListener("click", clearLocalData);
ui.notificationClose.addEventListener("click", () => { ui.notificationBanner.hidden = true; });
ui.notificationView.addEventListener("click", () => {
  const items = openItems();
  const index = items.findIndex((item) => item.id === notificationItemId);
  if (index >= 0) currentAttention = index;
  renderAttention();
  ui.notificationBanner.hidden = true;
  document.querySelector("#attention-heading").focus?.();
});
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!ui.setupOverlay.hidden) closeSetup();
  if (!ui.settingsOverlay.hidden) closeSettings();
});

(async () => {
  setConnected(false);
  try {
    await loadSnapshot();
    await loadSetup();
    await loadSettings();
    await recordUiMetric("app_opened");
    await loadSnapshot();
    knownAttentionIds = new Set(openItems().map((item) => item.id));
    notificationsPrimed = true;
    connectSocket();
  } catch (error) {
    ui.attentionList.replaceChildren(emptyState("!", "无法连接本地 Runtime", "请从 flow-agent serve 输出的一次性地址打开控制面板。"));
    showToast(`连接失败：${error.message}`);
  }
})();
