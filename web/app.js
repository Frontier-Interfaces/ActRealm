"use strict";

const ui = {
  actRealmWorkspace: document.querySelector("#actrealm-workspace"),
  menuClock: document.querySelector("#menu-clock"),
  runtimeState: document.querySelector("#runtime-state"),
  runtimeLabel: document.querySelector("#runtime-label"),
  agentStatusTrigger: document.querySelector("#agent-status-trigger"),
  agentStatusLabel: document.querySelector("#agent-status-label"),
  runtimeFooterLabel: document.querySelector("#runtime-footer-label"),
  runtimeSettingsLabel: document.querySelector("#runtime-settings-label"),
  offlineBanner: document.querySelector("#offline-banner"),
  attentionCount: document.querySelector("#attention-count"),
  attentionSummary: document.querySelector("#attention-summary"),
  attentionList: document.querySelector("#attention-list"),
  sessionCount: document.querySelector("#session-count"),
  sessionList: document.querySelector("#session-list"),
  quotaList: document.querySelector("#quota-list"),
  quotaSyncTime: document.querySelector("#quota-sync-time"),
  eventCount: document.querySelector("#event-count"),
  undoToast: document.querySelector("#undo-toast"),
  undoMessage: document.querySelector("#undo-message"),
  undoButton: document.querySelector("#undo-button"),
  toast: document.querySelector("#toast"),
  setupTrigger: document.querySelector("#setup-trigger"),
  setupTriggerLabel: document.querySelector("#setup-trigger-label"),
  setupOverlay: document.querySelector("#setup-overlay"),
  setupClose: document.querySelector("#setup-close"),
  setupProviders: document.querySelector("#setup-providers"),
  setupSummary: document.querySelector("#setup-summary"),
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
  codexConnectorStatus: document.querySelector("#codex-connector-status"),
  retentionDays: document.querySelector("#retention-days"),
  displayProfile: document.querySelector("#display-profile"),
  taskCardFields: document.querySelector("#task-card-fields"),
  claudeBridgeStatus: document.querySelector("#claude-bridge-status"),
  claudeBridgeAction: document.querySelector("#claude-bridge-action"),
  exportData: document.querySelector("#export-data"),
  clearData: document.querySelector("#clear-data"),
  metricsSummary: document.querySelector("#metrics-summary"),
  exportMetrics: document.querySelector("#export-metrics"),
  runtimeMonitor: document.querySelector("#runtime-monitor"),
  runtimeRestart: document.querySelector("#runtime-restart"),
  runtimeMonitorDetails: document.querySelector("#runtime-monitor-details"),
  runtimeMonitorGrid: document.querySelector("#runtime-monitor-grid"),
  runtimeMonitorRefresh: document.querySelector("#runtime-monitor-refresh"),
  reminderRows: [...document.querySelectorAll(".reminder-row[data-rule]")],
  retentionOptions: [...document.querySelectorAll(".retention-options button")],
  wipeConfirmation: document.querySelector("#wipe-confirmation"),
  wipeConfirmationInput: document.querySelector("#wipe-confirmation-input"),
  wipeConfirm: document.querySelector("#wipe-confirm"),
  wipeCancel: document.querySelector("#wipe-cancel"),
  notificationBanner: document.querySelector("#notification-banner"),
  notificationKind: document.querySelector("#notification-kind"),
  notificationTitle: document.querySelector("#notification-title"),
  notificationView: document.querySelector("#notification-view"),
  notificationClose: document.querySelector("#notification-close"),
  sessionDetailOverlay: document.querySelector("#session-detail-overlay"),
  sessionDetailClose: document.querySelector("#session-detail-close"),
  sessionDetailTitle: document.querySelector("#session-detail-title"),
  sessionDetailBody: document.querySelector("#session-detail-body"),
  sessionDetailJump: document.querySelector("#session-detail-jump"),
};

let csrfToken = sessionStorage.getItem("actrealm.csrf");
let snapshot = { sessions: [], attention: [], commands: [], quota: [], stats: {} };
let currentAttention = 0;
let socket;
let runtimeConnected = false;
let reconnectDelay = 500;
let undoCommandId;
let toastTimer;
let setupState = { providers: [], firstRun: false };
let setupLoaded = false;
let setupBusy = false;
let setupLoading = false;
let settingsState = {
  notificationRules: { approval: "list", question: "list", error: "list", completion: "list" },
  soundEnabled: true,
  providerMuted: { claude: false, codex: false },
  codexEnhancedActivity: true,
  retentionDays: 90,
  displayProfile: "detailed",
  taskCardFields: ["project", "task", "model", "activity", "plan", "tokens", "context", "tool", "subagents", "environment", "recovery", "control", "jump"],
};
let displayCatalog = [];
let claudeBridge = { status: "not_installed" };
let settingsBusy = false;
let notificationsPrimed = false;
let knownAttentionIds = new Set();
let notificationItemId;
let renderedEventCount = 0;
let eventUiLatencies = [];
let selectedSessionId;
let detailSessionId;
let sessionActivityRefs = new Map();
let sessionRenderSignatures = new Map();
let lastAttentionRenderSignature;
let lastQuotaRenderSignature;
let attentionExitTimer;
let reconnectTimer;
let setupRefreshTimer;
let lastSetupAt = 0;
let lastSocketFrameAt = 0;
let lastSnapshotAt = 0;
let fallbackSnapshotInFlight = false;
let restartInProgress = false;
let runtimeMonitorInFlight = false;
let lastRuntimeMonitorAt = 0;
let hiddenSessions = JSON.parse(localStorage.getItem("actrealm.hiddenSessions") || "{}");
const SESSION_VISIBLE_FOR_MS = 30 * 60 * 1000;
const SOCKET_STALE_AFTER_MS = 25 * 1000;
const SNAPSHOT_FALLBACK_AFTER_MS = 15 * 1000;
const SETUP_FOCUS_REFRESH_AFTER_MS = 5 * 1000;
const USER_GUIDE_URL = "https://github.com/Frontier-Interfaces/ActRealm/blob/agent/v1-full/docs/USER_GUIDE_zh-CN.md";
const DISPLAY_PRESETS = {
  concise: ["task", "activity"],
  detailed: ["project", "task", "model", "activity", "plan", "tokens", "context", "tool", "subagents", "environment", "recovery", "control", "jump"],
  developer: ["project", "task", "model", "activity", "plan", "tokens", "context", "tool", "permissionMode", "subagents", "environment", "recovery", "control", "jump", "titleSource", "sessionId", "providerSessionId", "providerTurnId", "lastEventAt"],
};

function updateClock() {
  const now = new Date();
  ui.menuClock.textContent = now.toLocaleTimeString([], { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
}

function providerIcon(provider) {
  const normalized = String(provider || "").toLowerCase();
  const source = {
    claude: "/assets/claude.png",
    codex: "/assets/codex.png",
  }[normalized];
  if (!source) return element("span", "provider-glyph provider-fallback", "?");
  const icon = element("img", `provider-glyph provider-${normalized}`);
  icon.src = source;
  icon.alt = `${providerName(normalized)} 图标`;
  icon.width = 28;
  icon.height = 28;
  return icon;
}

function emptyState(icon, title, detail) {
  const root = element("div", "empty-state");
  root.append(element("div", "empty-icon", icon));
  root.append(element("h3", "", title));
  root.append(element("p", "", detail));
  return root;
}

function onboardingTaskEmpty() {
  const root = element("div", "empty-state onboarding-task-empty");
  const icon = element("div", "onboarding-empty-icon");
  const signal = element("span", "signal-bars onboarding-signal");
  signal.append(element("i"), element("i"), element("i"));
  icon.append(signal);
  root.append(icon);
  root.append(element("h3", "", "尚未连接任何 Agent"));
  root.append(element("p", "", "连接 Claude 或 Codex 后，正在运行的任务与需要你处理的事会显示在这里。数据仅留在本机。"));
  const actions = element("div", "onboarding-actions");
  const connect = element("button", "onboarding-primary", "＋ 连接 Agent");
  connect.type = "button";
  connect.addEventListener("click", openSetup);
  actions.append(connect, guideLink("onboarding-guide"));
  root.append(actions);
  const providers = element("div", "onboarding-provider-shortcuts");
  for (const provider of setupState.providers || []) {
    const shortcut = element("span", "onboarding-provider-shortcut");
    shortcut.append(providerIcon(provider.provider), element("span", "", `连接 ${providerName(provider.provider)}`));
    providers.append(shortcut);
  }
  root.append(providers);
  return root;
}

function onboardingQuotaState() {
  const root = element("div", "onboarding-quota-list");
  for (const provider of setupState.providers || []) {
    const row = element("article", "quota-unavailable onboarding-quota-provider");
    const heading = element("div", "onboarding-quota-heading");
    heading.append(providerIcon(provider.provider), element("strong", "", providerName(provider.provider)));
    heading.append(element("span", "setup-status muted", "未接入"));
    row.append(heading);
    row.append(element("p", "", provider.status === "provider_missing"
      ? "尚未检测到桌面客户端或 CLI。"
      : "安全接入并产生真实会话后读取可验证额度。"));
    root.append(row);
  }
  const privacy = element("p", "onboarding-privacy", "数据仅在这台 Mac · 不发送遥测");
  root.append(privacy);
  return root;
}

function openItems() {
  const visibleStates = new Set(["open", "committing", "decision_sent"]);
  const weights = { error: 4, approval: 3, question: 2, completion: 1 };
  return snapshot.attention
    .filter((item) => visibleStates.has(item.state) && notificationRule(item) !== "ignore")
    .sort((a, b) => (weights[b.kind] || 0) - (weights[a.kind] || 0) || a.createdAt - b.createdAt);
}

function recentOutcome() {
  const finalStates = new Set(["confirmed", "resolved", "passed_through", "expired", "dismissed"]);
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

function guideLink(className = "", label = "查看接入指南") {
  const link = element("a", className, label);
  link.href = USER_GUIDE_URL;
  link.target = "_blank";
  link.rel = "noopener noreferrer";
  return link;
}

function isFirstRun() {
  return setupLoaded && Boolean(setupState.firstRun);
}

function setupStatus(status) {
  return {
    not_installed: { label: "未接入", className: "muted", detail: "不会修改现有配置，点击后先备份再语义合并。" },
    provider_missing: { label: "未找到客户端", className: "error", detail: "请先安装这个 Agent 的桌面客户端或命令行程序。" },
    cli_missing: { label: "未找到客户端", className: "error", detail: "请先安装这个 Agent 的桌面客户端或命令行程序。" },
    needs_trust: { label: "等待信任", className: "warning", detail: "打开 Codex，输入 /hooks，逐项检查并信任 ActRealm。" },
    installed_unverified: { label: "等待验证", className: "warning", detail: "配置已经就绪。启动一次真实会话后才能确认接入。" },
    connected: { label: "已接入", className: "ready", detail: "已收到安装后的真实 Agent 事件，实时活动可以正常显示。" },
    needs_reinstall: { label: "配置有变化", className: "error", detail: "发现不完整或被修改的 ActRealm 条目；不会自动覆盖。" },
    inline_conflict: { label: "配置冲突", className: "error", detail: "Codex 同时存在 inline Hook。请先保留一种同层配置形式。" },
    error: { label: "配置无法解析", className: "error", detail: "为保护你的设置，ActRealm 已拒绝改写。请先恢复或修正配置。" },
  }[status] || { label: status, className: "muted", detail: "状态暂时无法识别。" };
}

function setupButton(label, className, handler, disabled = false) {
  const button = element("button", `setup-action ${className || ""}`.trim(), label);
  button.type = "button";
  button.disabled = disabled || setupBusy || !runtimeConnected;
  button.addEventListener("click", handler);
  return button;
}

function setupDetectedText(provider) {
  if (provider.cliInstalled && provider.desktopInstalled) return "检测到桌面客户端与 CLI";
  if (provider.desktopInstalled) return "检测到桌面客户端 · 不要求全局 CLI";
  if (provider.cliInstalled) return "检测到 CLI";
  return "尚未检测到可用客户端";
}

async function copyCodexTrustCommand(provider) {
  const command = provider.reviewCommand;
  if (!command) {
    showToast("没有检测到可用的 Codex 启动命令，请查看接入指南");
    return;
  }
  try {
    await navigator.clipboard.writeText(command);
    showToast("Codex 启动命令已复制；请在终端运行后输入 /hooks");
  } catch (_) {
    showToast("复制失败；请手动复制卡片中的 Codex 启动命令");
  }
}

function renderSetupSummary() {
  const providers = setupState.providers || [];
  const connected = providers.filter((provider) => provider.status === "connected").length;
  const pending = providers.filter((provider) => !["connected", "not_installed", "provider_missing", "cli_missing"].includes(provider.status)).length;
  ui.setupSummary.replaceChildren(
    element("span", "setup-summary-ready", `已接入 ${connected}`),
    element("span", pending ? "setup-summary-pending" : "setup-summary-muted", `待处理 ${pending}`),
  );
}

function renderSetupHeader() {
  const providers = setupState.providers || [];
  const connected = providers.filter((provider) => provider.status === "connected").length;
  const pending = providers.filter((provider) => !["connected", "not_installed", "provider_missing", "cli_missing"].includes(provider.status)).length;
  ui.agentStatusTrigger.classList.toggle("connected", connected > 0 && pending === 0);
  ui.agentStatusTrigger.classList.toggle("needs-attention", pending > 0);
  if (!setupLoaded) {
    ui.agentStatusLabel.textContent = "正在检测 Agent";
    ui.setupTriggerLabel.textContent = "连接 Agent";
  } else if (isFirstRun()) {
    ui.agentStatusLabel.textContent = "未连接 Agent";
    ui.setupTriggerLabel.textContent = "连接 Agent";
  } else if (pending > 0) {
    ui.agentStatusLabel.textContent = connected ? `${connected} 个已接入 · ${pending} 待处理` : `${pending} 项接入待处理`;
    ui.setupTriggerLabel.textContent = "完成接入";
  } else {
    ui.agentStatusLabel.textContent = `${connected} 个 Agent 已接入`;
    ui.setupTriggerLabel.textContent = "Agent 接入";
  }
  ui.setupTrigger.classList.toggle("needs-attention", setupLoaded && (isFirstRun() || pending > 0));
}

function renderSetup() {
  ui.setupProviders.replaceChildren();
  renderSetupSummary();
  for (const provider of setupState.providers || []) {
    const status = setupStatus(provider.status);
    const card = element("article", "setup-provider-row");
    const identity = element("div", "setup-identity");
    identity.append(providerIcon(provider.provider));
    const identityCopy = element("div", "setup-identity-copy");
    identityCopy.append(
      element("strong", "", provider.provider === "claude" ? "Claude Code" : "Codex"),
      element("span", "", setupDetectedText(provider)),
    );
    identity.append(identityCopy);

    const config = element("div", "setup-config");
    config.append(element("span", "", "配置"));
    const configPath = element("code", "", provider.configPath || "尚未生成配置路径");
    configPath.title = provider.configPath || "";
    config.append(configPath);

    const state = element("div", "setup-provider-state");
    state.append(element("span", `setup-status ${status.className}`, status.label));
    state.append(element("p", "setup-detail", status.detail));

    const actions = element("div", "setup-actions");
    if (provider.canRepair) {
      actions.append(setupButton("修复二进制", "primary", () => changeSetup(provider.provider, "repair")));
    } else if (provider.status === "not_installed") {
      actions.append(setupButton("安全接入", "primary", () => changeSetup(provider.provider, "install")));
    } else if (provider.status === "needs_reinstall") {
      actions.append(setupButton("检查后重新安装", "primary", () => changeSetup(provider.provider, "install")));
    } else if (provider.status === "needs_trust") {
      if (provider.reviewCommand) actions.append(setupButton("复制信任命令", "primary", () => copyCodexTrustCommand(provider)));
      actions.append(setupButton("刷新状态", "ghost", loadSetup));
      actions.append(setupButton("移除接入", "danger", () => changeSetup(provider.provider, "uninstall")));
    } else if (["installed_unverified", "connected"].includes(provider.status)) {
      actions.append(setupButton("刷新状态", "primary", loadSetup));
      actions.append(setupButton("移除接入", "danger", () => changeSetup(provider.provider, "uninstall")));
    } else if (["inline_conflict", "error"].includes(provider.status)) {
      actions.append(setupButton("重新检测", "ghost", loadSetup));
    } else if (["provider_missing", "cli_missing"].includes(provider.status)) {
      actions.append(guideLink("setup-action ghost", "查看安装说明"));
    } else {
      actions.append(setupButton("重新检测", "ghost", loadSetup));
    }

    card.append(identity, config, state, actions);

    if (provider.provider === "codex" && provider.status === "needs_trust") {
      const trust = element("div", "setup-trust");
      trust.append(element("strong", "", "Codex 信任必须由你在官方界面确认"));
      const steps = element("ol", "trust-steps");
      const startStep = provider.cliInstalled
        ? "打开任意 Codex 终端会话"
        : `打开终端并运行内置 Codex：${provider.reviewCommand || "ChatGPT.app/Contents/Resources/codex"}`;
      for (const step of [startStep, "输入 /hooks", "核对命令路径后选择信任", "启动一个新会话并回到这里刷新"]) {
        steps.append(element("li", "", step));
      }
      trust.append(steps);
      if (provider.reviewCommand) trust.append(element("code", "setup-review-command", provider.reviewCommand));
      card.append(trust);
    }
    ui.setupProviders.append(card);
  }
  renderSetupHeader();
}

function openSetup() {
  ui.actRealmWorkspace.hidden = true;
  ui.settingsOverlay.hidden = true;
  ui.setupOverlay.hidden = false;
  ui.setupClose.focus();
  void loadSetup();
}

function closeSetup() {
  ui.setupOverlay.hidden = true;
  ui.actRealmWorkspace.hidden = false;
  ui.setupTrigger.focus();
}

async function loadSetup() {
  if (setupLoading) return;
  setupLoading = true;
  try {
    setupState = await api("/api/v1/setup");
    setupLoaded = true;
    lastSetupAt = Date.now();
    renderSetup();
    renderOnboardingState();
  } catch (error) {
    showToast(`接入状态读取失败：${error.message}`);
  } finally {
    setupLoading = false;
  }
}

async function changeSetup(provider, action) {
  if (setupBusy) return;
  setupBusy = true;
  renderSetup();
  try {
    setupState = await api("/api/v1/setup", {
      method: "POST",
      body: JSON.stringify({
        provider,
        action,
        enhancedCodexActivity: Boolean(settingsState.codexEnhancedActivity),
      }),
    });
    setupLoaded = true;
    lastSetupAt = Date.now();
    renderSetup();
    renderOnboardingState();
    showToast(action === "uninstall" ? `${providerName(provider)} 接入已移除` : `${providerName(provider)} 配置已安全写入`);
  } catch (error) {
    showToast(`接入操作失败：${error.detail || error.message}`);
  } finally {
    setupBusy = false;
    renderSetup();
  }
}

function renderOnboardingState() {
  document.body.classList.toggle("first-run", isFirstRun());
  renderSetupHeader();
  lastAttentionRenderSignature = undefined;
  lastQuotaRenderSignature = undefined;
  renderAttention();
  renderSessions();
  renderQuota();
}

function openSettings() {
  ui.setupOverlay.hidden = true;
  ui.actRealmWorkspace.hidden = true;
  ui.settingsOverlay.hidden = false;
  renderMetrics();
  ui.settingsClose.focus();
  loadSettings();
}

function closeSettings() {
  ui.settingsOverlay.hidden = true;
  ui.actRealmWorkspace.hidden = false;
  ui.settingsTrigger.focus();
}

function bridgeStatusCopy(status) {
  return {
    installed: "已开启 · 等待 Claude 下一次响应更新",
    not_installed: "未开启",
    helper_missing: "桥接文件缺失，可安全修复",
    custom_conflict: "检测到自定义状态栏；可以保留原显示并串联额度采集",
    config_malformed: "Claude 配置无法解析，已停止修改",
  }[status] || "状态暂时不可用";
}

function renderSettings() {
  const rules = settingsState.notificationRules || {};
  const visibleRule = (value) => value === "ignore" ? "ignore" : "list";
  ui.notifyApproval.value = visibleRule(rules.approval);
  ui.notifyQuestion.value = visibleRule(rules.question);
  ui.notifyError.value = visibleRule(rules.error);
  ui.notifyCompletion.value = visibleRule(rules.completion);
  ui.soundEnabled.checked = Boolean(settingsState.soundEnabled);
  ui.muteClaude.checked = Boolean(settingsState.providerMuted?.claude);
  ui.muteCodex.checked = Boolean(settingsState.providerMuted?.codex);
  ui.codexEnhanced.checked = Boolean(settingsState.codexEnhancedActivity);
  const connector = snapshot.capabilities?.codexConnector;
  ui.codexConnectorStatus.textContent = connector?.status === "connected"
    ? `已连接 · ${connector.managedThreads || 0} 个托管对话`
    : connector?.status === "disabled"
      ? "当前 Runtime 未启用"
      : connector?.error || "当前版本不可用";
  const retention = [30, 90, 180, 0].includes(Number(settingsState.retentionDays))
    ? Number(settingsState.retentionDays)
    : 180;
  ui.retentionDays.value = String(retention);
  ui.displayProfile.value = settingsState.displayProfile || "detailed";
  renderFieldSelector();
  ui.claudeBridgeStatus.textContent = bridgeStatusCopy(claudeBridge.status);
  const removable = claudeBridge.status === "installed";
  const blocked = claudeBridge.status === "config_malformed";
  ui.claudeBridgeAction.textContent = removable
    ? "关闭"
    : claudeBridge.status === "custom_conflict"
      ? "保留现有并开启"
      : claudeBridge.status === "helper_missing"
        ? "修复"
        : "开启";
  ui.claudeBridgeAction.dataset.action = removable
    ? "uninstall"
    : claudeBridge.status === "custom_conflict"
      ? "wrap"
      : "install";
  ui.claudeBridgeAction.disabled = settingsBusy || blocked;
  for (const row of ui.reminderRows) {
    const key = row.dataset.rule;
    const value = visibleRule(rules[key]);
    for (const button of row.querySelectorAll("button[data-value]")) {
      button.classList.toggle("active", button.dataset.value === value);
      button.setAttribute("aria-pressed", String(button.dataset.value === value));
    }
  }
  for (const button of ui.retentionOptions) {
    const active = Number(button.dataset.value) === retention;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  }
}

function renderFieldSelector() {
  ui.taskCardFields.replaceChildren();
  const selected = new Set(settingsState.taskCardFields || []);
  const profile = settingsState.displayProfile || "detailed";
  for (const field of displayCatalog) {
    const label = element("label", `field-option field-${field.level || "detailed"}`);
    const input = document.createElement("input");
    input.type = "checkbox";
    input.value = field.id;
    input.checked = selected.has(field.id);
    input.disabled = field.level === "developer" && profile !== "developer";
    label.append(input, element("span", "", field.label));
    ui.taskCardFields.append(label);
  }
}

async function loadSettings() {
  try {
    const response = await api("/api/v1/settings");
    settingsState = response.settings;
    displayCatalog = response.displayCatalog || [];
    claudeBridge = response.claudeQuotaBridge;
    renderSettings();
    renderAttention();
    renderSessions();
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
    displayProfile: ui.displayProfile.value,
    taskCardFields: [...ui.taskCardFields.querySelectorAll("input:checked")].map((input) => input.value),
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
    displayCatalog = response.displayCatalog || displayCatalog;
    claudeBridge = response.claudeQuotaBridge;
    renderSettings();
    renderAttention();
    renderSessions();
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
    showToast(action === "uninstall" ? "Claude 额度桥已关闭，原状态栏已恢复" : "Claude 额度桥已开启，完成一次对话后会显示额度");
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
    link.download = "actrealm-export.json";
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
    link.download = "actrealm-metrics.json";
    document.body.append(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
    showToast("仅统计数据已导出，不含会话和事件明细");
  } catch (error) {
    showToast(`统计导出失败：${error.message}`);
  }
}

function openClearConfirmation() {
  ui.wipeConfirmation.hidden = false;
  ui.wipeConfirmationInput.value = "";
  ui.wipeConfirmationInput.focus();
}

function cancelClearConfirmation() {
  ui.wipeConfirmation.hidden = true;
  ui.wipeConfirmationInput.value = "";
}

async function clearLocalData() {
  const confirmation = ui.wipeConfirmationInput.value.trim();
  if (confirmation !== "DELETE") {
    showToast("请输入 DELETE；没有删除任何数据");
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
    cancelClearConfirmation();
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
    dismissed: "已忽略",
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
  if (item.kind === "native_approval") return item.title || `${providerName(item.provider)} 需要你在原界面批准`;
  if (item.kind === "error") return item.title || "任务出错停下来了";
  if (item.kind === "completion") return item.title || "这一轮已经完成";
  return item.title || "Agent 有一件事需要你处理";
}

function attentionContext(item) {
  if (item.kind === "native_approval") {
    return {
      kicker: `${providerName(item.provider)} 原界面请求 · ActRealm 仅同步状态`,
      state: "等待原界面处理",
      notification: "原界面请求批准",
    };
  }
  if (item.kind === "approval") {
    return {
      kicker: "可在 ActRealm 审批 · 任务等待决定",
      state: stateLabel(item.state),
      notification: "可在 ActRealm 审批",
    };
  }
  if (item.kind === "question") {
    return {
      kicker: "可在 ActRealm 回答 · 任务等待输入",
      state: stateLabel(item.state),
      notification: "等待回答",
    };
  }
  return {
    kicker: item.kind === "completion" ? "任务已完成 · 不着急" : "需要你处理 · 任务已暂停",
    state: stateLabel(item.state),
    notification: stateLabel(item.kind),
  };
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

async function submitQuestion(item, submission, controls) {
  for (const control of controls) control.disabled = true;
  try {
    await api(`/api/v1/questions/${encodeURIComponent(item.requestId)}/answer`, {
      method: "POST",
      body: JSON.stringify(submission),
    });
    showToast(submission.action === "native" ? "已交回 Agent 原界面回答" : "回答已安全发送给 Agent");
    await loadSnapshot();
  } catch (error) {
    for (const control of controls) control.disabled = false;
    showToast(error.message === "QUESTION_EXPIRED" ? "这个问题已经过期，不能再提交" : `回答失败：${error.message}`);
  }
}

function renderInteractiveForm(item, card) {
  const interaction = item.interaction;
  if (!interaction || item.state !== "open" || !item.requestId) return false;
  if (interaction.message) card.append(element("div", "fact-block question-message", interaction.message));
  const form = element("form", "question-form");
  const bindings = [];
  const allControls = [];
  for (const question of interaction.questions || []) {
    const fieldset = element("fieldset", "question-field");
    const legend = element("legend", "", question.label || "问题");
    fieldset.append(legend);
    if (question.prompt) fieldset.append(element("p", "question-prompt", question.prompt));
    const binding = { question, values: [], other: undefined, input: undefined };
    if (question.inputType === "choice") {
      const choices = element("div", "question-choices");
      for (const [index, option] of (question.options || []).entries()) {
        const label = element("label", "question-choice");
        const input = document.createElement("input");
        input.type = question.multiSelect ? "checkbox" : "radio";
        input.name = `answer-${item.requestId}-${question.id}`;
        input.value = option.label;
        input.id = `answer-${item.requestId}-${question.id}-${index}`;
        allControls.push(input);
        binding.values.push(input);
        const copy = element("span", "");
        copy.append(element("strong", "", option.label));
        if (option.description) copy.append(element("small", "", option.description));
        label.append(input, copy);
        choices.append(label);
      }
      if (question.allowsOther) {
        const other = document.createElement("input");
        other.type = "text";
        other.className = "question-other";
        other.placeholder = "其他答案（可直接输入）";
        other.maxLength = 2000;
        allControls.push(other);
        binding.other = other;
        choices.append(other);
      }
      fieldset.append(choices);
    } else if (question.inputType === "boolean") {
      const input = document.createElement("select");
      input.append(new Option("请选择", ""), new Option("是", "true"), new Option("否", "false"));
      allControls.push(input);
      binding.input = input;
      fieldset.append(input);
    } else {
      const input = document.createElement("input");
      input.type = question.isSecret ? "password" : question.inputType === "number" ? "number" : "text";
      input.autocomplete = question.isSecret ? "new-password" : "off";
      input.maxLength = 8192;
      allControls.push(input);
      binding.input = input;
      fieldset.append(input);
      if (question.isSecret) fieldset.append(element("p", "secret-note", "仅在内存中提交，不写入数据库、日志或导出。"));
    }
    bindings.push(binding);
    form.append(fieldset);
  }
  const actions = element("div", "actions question-actions");
  const submit = element("button", "action-button", "发送回答");
  submit.type = "submit";
  allControls.push(submit);
  actions.append(submit);
  if (interaction.kind === "claude_elicitation") {
    for (const [label, action] of [["拒绝提供", "decline"], ["取消请求", "cancel"]]) {
      const button = element("button", "action-button ghost", label);
      button.type = "button";
      button.addEventListener("click", () => submitQuestion(item, { action }, allControls));
      allControls.push(button);
      actions.append(button);
    }
  }
  if (interaction.supportsNative) {
    const native = element("button", "action-button ghost", "去 Agent 回答");
    native.type = "button";
    native.addEventListener("click", () => submitQuestion(item, { action: "native" }, allControls));
    allControls.push(native);
    actions.append(native);
  }
  form.append(actions);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const answers = {};
    for (const binding of bindings) {
      const { question } = binding;
      if (question.inputType === "choice") {
        const values = binding.values.filter((input) => input.checked).map((input) => input.value);
        if (binding.other?.value.trim()) values.push(binding.other.value.trim());
        if (!values.length && question.required) {
          showToast(`请回答“${question.label}”`);
          return;
        }
        answers[question.id] = ["claude_question", "codex_user_input"].includes(interaction.kind) ? values : values[0];
      } else if (question.inputType === "boolean") {
        if (!binding.input.value && question.required) {
          showToast(`请回答“${question.label}”`);
          return;
        }
        if (binding.input.value) answers[question.id] = interaction.kind === "codex_user_input" ? [binding.input.value] : binding.input.value === "true";
      } else {
        const value = binding.input.value.trim();
        if (!value && question.required) {
          showToast(`请填写“${question.label}”`);
          return;
        }
        if (value) {
          const normalized = question.inputType === "number" ? Number(value) : value;
          answers[question.id] = interaction.kind === "codex_user_input" ? [String(normalized)] : normalized;
        }
      }
    }
    submitQuestion(item, { action: "accept", answers }, allControls);
  });
  card.append(form);
  return true;
}

function attentionRenderSignature() {
  const items = openItems();
  const sessionIds = new Set(items.map((item) => item.sessionId));
  return JSON.stringify({
    currentAttention,
    items,
    commands: snapshot.commands.filter((command) => items.some((item) => item.id === command.attentionId)),
    sessions: snapshot.sessions
      .filter((session) => sessionIds.has(session.id))
      .map((session) => ({ id: session.id, title: session.title, providerTitle: session.providerTitle, project: session.project })),
  });
}

function updateAttentionTimes() {
  const items = openItems();
  if (items.length) {
    const oldest = Math.min(...items.map((item) => Number(item.createdAt || Date.now())));
    const minutes = Math.max(0, Math.floor((Date.now() - oldest) / 60000));
    ui.attentionSummary.textContent = `最久等待 ${minutes} 分钟`;
  } else {
    ui.attentionSummary.textContent = "没有需要你处理的事项";
  }
  updateMarkedElapsed(ui.attentionList);
}

function renderAttention() {
  lastAttentionRenderSignature = attentionRenderSignature();
  const items = openItems();
  ui.attentionCount.textContent = String(items.length);
  ui.attentionList.replaceChildren();
  updateAttentionTimes();
  if (!items.length) {
    ui.attentionList.append(isFirstRun()
      ? emptyState("⌑", "还没有需要处理的事项", "连接 Agent 后，审批、提问和完成确认会出现在这里。")
      : emptyState("✓", "全部处理完毕", "新的授权、问题、完成或错误会实时进入 OUTBOX。"));
    return;
  }
  currentAttention = Math.min(currentAttention, items.length - 1);
  const item = items[currentAttention];
  const context = attentionContext(item);
  const card = element("article", `attention-card ${item.kind || "approval"}`);
  const kicker = element("div", "attention-kicker");
  const kindLabel = {
    approval: "等待批准",
    native_approval: "原界面批准",
    question: "提问",
    completion: "完成",
    error: "错误",
  }[item.kind] || "待处理";
  kicker.append(element("span", "attention-kind", kindLabel));
  kicker.append(markLiveElapsed(element("span", "attention-state"), item.createdAt, "已等 "));
  card.append(kicker, element("h3", "", attentionTitle(item)));

  const agentLine = element("div", "agent-line");
  agentLine.append(providerIcon(item.provider));
  agentLine.append(element("strong", "", providerName(item.provider)));
  if (item.project) agentLine.append(element("span", "", `· ${item.project}`));
  const session = snapshot.sessions.find((candidate) => candidate.id === item.sessionId);
  if (session?.title) agentLine.append(element("span", "", session.title));
  card.append(agentLine);

  const taskJump = element("button", "task-jump", "在 Agent 任务中查看 →");
  taskJump.type = "button";
  taskJump.addEventListener("click", () => selectSession(item.sessionId));
  card.append(taskJump);

  const interactive = renderInteractiveForm(item, card);
  if (!interactive) {
    const fact = item.detail || item.commandPreview;
    if (fact) card.append(element("div", "fact-block", fact));
    const risk = element("div", "risk-row");
    risk.append(element("span", "risk-chip", `风险标记：${item.risk || "未知"}`));
    for (const note of item.riskNotes || []) risk.append(element("span", "risk-chip", note));
    if (item.expiresAt) risk.append(element("span", "risk-chip", `截止 ${new Date(item.expiresAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`));
    card.append(risk);
  }

  const actions = element("div", "actions");
  if (interactive) {
    // The interactive form owns its submit, decline, cancel, and handoff actions.
  } else if (item.state === "open" && item.kind === "native_approval") {
    const openProvider = element("button", "action-button", "返回原窗口");
    openProvider.type = "button";
    openProvider.addEventListener("click", () => {
      if (session) activateSession(session);
      else selectSession(item.sessionId);
    });
    actions.append(openProvider);
  } else if (item.state === "open" && item.kind === "approval") {
    actions.append(actionButton("允许", "", "approve", item));
    actions.append(actionButton("拒绝", "deny", "deny", item));
    const confirm = element("button", "action-button ghost", "二次确认后允许");
    confirm.type = "button";
    confirm.addEventListener("click", () => {
      actions.replaceChildren();
      actions.append(element("strong", "confirm-copy", "确认允许运行这项操作？"));
      actions.append(actionButton("确认允许", "", "approve", item));
      const cancel = element("button", "action-button ghost", "取消");
      cancel.type = "button";
      cancel.addEventListener("click", renderAttention);
      actions.append(cancel);
    });
    actions.append(confirm);
  } else if (item.state === "open") {
    const acknowledge = item.kind === "completion" ? "确认完成" : "标记已解决";
    actions.append(actionButton(acknowledge, "", "ack", item));
    if (item.kind === "completion" && session) {
      const back = element("button", "action-button ghost", "返回原窗口");
      back.type = "button";
      back.addEventListener("click", () => activateSession(session));
      actions.append(back);
    }
  } else if (item.state === "committing") {
    const command = latestCommand(item);
    if (command && command.state === "pending_commit") {
      const undo = element("button", "action-button ghost", "撤回决定");
      undo.type = "button";
      undo.addEventListener("click", () => undoCommand(command.id));
      actions.append(undo);
    }
  }
  if (!interactive) card.append(actions);
  ui.attentionList.append(card);
  if (items.length > 1) {
    ui.attentionList.append(element("div", "queue-label", `队列 · 还有 ${items.length - 1} 项`));
    const queue = element("div", "attention-queue");
    items.forEach((candidate, index) => {
      if (index === currentAttention) return;
      const row = element("button", "queue-item");
      row.type = "button";
      row.append(
        element("span", `queue-kind ${candidate.kind || "approval"}`, {
          approval: "等待批准", native_approval: "原界面批准", question: "提问", completion: "完成", error: "错误",
        }[candidate.kind] || "待处理"),
        element("strong", "", attentionTitle(candidate)),
        markLiveElapsed(element("span", ""), candidate.createdAt),
      );
      row.addEventListener("click", () => { currentAttention = index; renderAttention(); });
      queue.append(row);
    });
    ui.attentionList.append(queue);
  }
}

function sessionStatus(session) {
  const waiting = blockingAttentionForSession(session);
  if (waiting.length) {
    const first = waiting[0];
    const suffix = waiting.length > 1 ? ` ×${waiting.length}` : "";
    if (first.kind === "native_approval") return { label: `原界面请求${suffix}`, className: "waiting" };
    if (first.kind === "approval") return { label: `面板可审批${suffix}`, className: "waiting" };
    if (first.kind === "question") return { label: `等你回答${suffix}`, className: "waiting" };
    if (first.kind === "completion") return { label: `待确认${suffix}`, className: "waiting" };
    return { label: `待处理${suffix}`, className: "waiting" };
  }
  if (session.execState === "failed") return { label: "出错", className: "failed" };
  const completion = pendingAttentionForSession(session).find((item) => item.kind === "completion");
  if (!isSessionActive(session) && completion) return { label: "待确认", className: "waiting" };
  if (["idle", "response_finished"].includes(session.execState)) return { label: "空闲", className: "idle" };
  return { label: "在跑", className: "" };
}

function recoveryDisplay(session) {
  return {
    controllable: { label: "已重新连接，可控制", className: "controllable" },
    observing: { label: "仍在运行，仅可观察", className: "observing" },
    waiting_for_event: { label: "历史已恢复，等待新事件", className: "waiting" },
    lost_control: { label: "已失去控制", className: "lost" },
    ended: { label: "已结束", className: "ended" },
  }[session.recoveryState] || { label: "等待确认状态", className: "waiting" };
}

function elapsedText(since, until = Date.now()) {
  const seconds = Math.max(0, Math.floor((Number(until) - Number(since || until)) / 1000));
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分 ${seconds % 60} 秒`;
  return `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分`;
}

function markLiveElapsed(node, since, prefix = "", suffix = "") {
  node.dataset.liveElapsedSince = String(Number(since || Date.now()));
  node.dataset.liveElapsedPrefix = prefix;
  node.dataset.liveElapsedSuffix = suffix;
  node.textContent = `${prefix}${elapsedText(node.dataset.liveElapsedSince)}${suffix}`;
  return node;
}

function updateMarkedElapsed(root) {
  for (const node of root.querySelectorAll("[data-live-elapsed-since]")) {
    node.textContent = `${node.dataset.liveElapsedPrefix || ""}${elapsedText(node.dataset.liveElapsedSince)}${node.dataset.liveElapsedSuffix || ""}`;
  }
}

function isSessionActive(session) {
  return !["idle", "response_finished", "failed"].includes(session.execState);
}

function pendingAttentionForSession(session) {
  return snapshot.attention
    .filter((item) => item.sessionId === session.id && ["open", "committing", "decision_sent"].includes(item.state))
    .sort((left, right) => left.createdAt - right.createdAt);
}

function blockingAttentionForSession(session) {
  return pendingAttentionForSession(session)
    .filter((item) => ["approval", "native_approval", "question"].includes(item.kind));
}

function turnTiming(session) {
  const started = Number(session.turnStartedAt || session.activitySince || session.lastEventAt);
  const ended = Number(session.turnEndedAt || Date.now());
  const total = elapsedText(started, ended);
  const active = !session.turnEndedAt && !["idle", "response_finished", "failed"].includes(session.execState);
  const stage = active && Number(session.activitySince || 0) > started
    ? ` · 当前阶段 ${elapsedText(session.activitySince)}`
    : "";
  return `本轮 ${total}${stage}`;
}

function compactCount(value) {
  const count = Number(value || 0);
  if (count >= 1_000_000) return `${Math.round(count / 100_000) / 10}m`;
  if (count >= 1_000) return `${Math.round(count / 100) / 10}k`;
  return String(count);
}

function contextUsage(session) {
  const used = Number(session.contextUsedTokens || 0);
  const windowSize = Number(session.contextWindowTokens || 0);
  const providerPercent = Number(session.contextUsedPercent);
  const percent = Number.isFinite(providerPercent) && session.contextUsedPercent != null
    ? Math.max(0, Math.min(100, providerPercent))
    : used > 0 && windowSize > 0
      ? Math.max(0, Math.min(100, Math.round(used / windowSize * 100)))
      : undefined;
  return { used, windowSize, percent };
}

function contextUsageText(session) {
  const context = contextUsage(session);
  if (!context.windowSize && context.percent == null) return "—";
  if (context.used > 0 && context.windowSize > 0) {
    return `${compactCount(context.used)} / ${compactCount(context.windowSize)} · ${context.percent}%`;
  }
  return context.percent == null ? `0 / ${compactCount(context.windowSize)}` : `${context.percent}%`;
}

function estimatedCostText(session) {
  if (session.estimatedCostUsdMicros == null) return undefined;
  const dollars = Number(session.estimatedCostUsdMicros) / 1_000_000;
  if (!Number.isFinite(dollars) || dollars < 0) return undefined;
  if (dollars > 0 && dollars < 0.01) return `$${dollars.toFixed(4)}`;
  return `$${dollars.toFixed(2)}`;
}

function appendUsageStrip(container, session) {
  const total = session.tokenTotal == null ? undefined : compactCount(session.tokenTotal);
  const context = contextUsage(session);
  const cost = estimatedCostText(session);
  if (total == null && context.percent == null && cost == null) return;
  const strip = element("div", "session-usage-strip");
  if (total != null) strip.append(element("span", "usage-chip", `累计 ${total} Token`));
  if (context.percent != null) {
    const chip = element("span", "usage-chip context", `上下文 ${context.percent}%`);
    chip.title = contextUsageText(session);
    strip.append(chip);
  }
  if (cost != null) {
    const chip = element("span", "usage-chip cost", `估算 API 价 ${cost}`);
    chip.title = "按公开 API 单价或 Provider 官方会话费用估算；不是订阅账单";
    strip.append(chip);
  }
  container.append(strip);
}

function cardFieldVisible(field) {
  return (settingsState.taskCardFields || []).includes(field);
}

function detailPair(label, value, className = "") {
  if (value === undefined || value === null || value === "") return undefined;
  const row = element("div", `detail-pair ${className}`.trim());
  row.append(element("span", "", label), element("strong", "", value));
  return row;
}

function closeSessionDetail() {
  ui.sessionDetailOverlay.hidden = true;
  const sessionId = detailSessionId;
  detailSessionId = undefined;
  if (sessionId) {
    ui.sessionList.querySelector(`[data-session-id="${CSS.escape(sessionId)}"] .session-details`)?.focus();
  }
}

function openSessionDetail(session) {
  detailSessionId = session.id;
  ui.sessionDetailTitle.textContent = session.providerTitle || session.title || "任务详情";
  ui.sessionDetailBody.replaceChildren();
  const fields = new Set(settingsState.taskCardFields || []);
  const status = sessionStatus(session);
  const recovery = recoveryDisplay(session);
  const rows = [
    detailPair("Provider", providerName(session.provider)),
    detailPair("状态", status.label),
    fields.has("recovery") ? detailPair("恢复状态", recovery.label) : undefined,
    fields.has("control") ? detailPair("控制能力", session.controlCapability === "managed" ? "Codex app-server 托管，可回答提问" : "外部 Hook，仅观察/授权") : undefined,
    fields.has("project") ? detailPair("项目", session.project) : undefined,
    fields.has("task") ? detailPair("任务", session.title) : undefined,
    fields.has("model") ? detailPair("模型", session.model) : undefined,
    fields.has("activity") ? detailPair("实时活动", activityDisplay(session).text) : undefined,
    fields.has("plan") && Number.isInteger(session.planTotal) && session.planTotal > 0
      ? detailPair("计划", `${session.planDone || 0}/${session.planTotal}`)
      : undefined,
    fields.has("tokens") && session.tokenTotal !== undefined && session.tokenTotal !== null
      ? detailPair("会话累计 Token", compactCount(session.tokenTotal))
      : undefined,
    fields.has("context") && Number(session.contextWindowTokens) > 0
      ? detailPair("本轮上下文", contextUsageText(session))
      : undefined,
    fields.has("tokens") ? detailPair("输入 / 输出", session.inputTokens == null && session.outputTokens == null
      ? undefined
      : `${compactCount(session.inputTokens || 0)} / ${compactCount(session.outputTokens || 0)}`) : undefined,
    fields.has("tokens") ? detailPair("缓存读取 / 写入", session.cacheReadTokens == null && session.cacheCreationTokens == null
      ? undefined
      : `${compactCount(session.cacheReadTokens || 0)} / ${compactCount(session.cacheCreationTokens || 0)}`) : undefined,
    fields.has("tokens") ? detailPair("推理 Token", session.reasoningTokens == null ? undefined : compactCount(session.reasoningTokens)) : undefined,
    fields.has("tokens") ? detailPair("本轮 Token", session.lastTurnTokens == null ? undefined : compactCount(session.lastTurnTokens)) : undefined,
    fields.has("tokens") ? detailPair("估算 API 价", estimatedCostText(session)) : undefined,
    fields.has("tool") ? detailPair("当前工具", session.currentTool) : undefined,
    fields.has("permissionMode") ? detailPair("权限模式", session.permissionMode) : undefined,
    fields.has("subagents") ? detailPair("运行中的子 Agent", String(session.activeSubagents || 0)) : undefined,
    fields.has("environment") ? detailPair("运行环境", session.environment) : undefined,
    fields.has("jump") ? detailPair("跳转能力", session.jumpLabel) : undefined,
    fields.has("titleSource") ? detailPair("标题来源", session.providerTitleSource) : undefined,
    fields.has("sessionId") ? detailPair("ActRealm Session ID", session.id, "developer-value") : undefined,
    fields.has("providerSessionId") ? detailPair("Provider Session ID", session.providerSessionId, "developer-value") : undefined,
    fields.has("providerTurnId") ? detailPair("Provider Turn ID", session.providerTurnId, "developer-value") : undefined,
    fields.has("lastEventAt") ? detailPair("最后事件", new Date(session.lastEventAt).toLocaleString()) : undefined,
  ].filter(Boolean);
  const grid = element("div", "session-detail-grid");
  for (const row of rows) grid.append(row);
  ui.sessionDetailBody.append(grid);
  ui.sessionDetailJump.textContent = session.jumpLabel || "当前环境不支持跳转";
  ui.sessionDetailJump.disabled = session.jumpCapability === "unsupported";
  ui.sessionDetailJump.onclick = () => jumpSession(session);
  ui.sessionDetailOverlay.hidden = false;
  ui.sessionDetailClose.focus();
}

function visibleSessions() {
  const attentionSessions = new Set(
    snapshot.attention
      .filter((item) => ["open", "committing", "decision_sent", "snoozed"].includes(item.state))
      .map((item) => item.sessionId),
  );
  const cutoff = Date.now() - SESSION_VISIBLE_FOR_MS;
  return snapshot.sessions
    .filter((session) => {
      const hiddenAt = Number(hiddenSessions[session.id] || 0);
      if (hiddenAt && Number(session.lastEventAt || 0) <= hiddenAt) return false;
      const active = !["idle", "response_finished", "failed"].includes(session.execState);
      return active || Number(session.lastEventAt || 0) >= cutoff || attentionSessions.has(session.id);
    })
    .sort((a, b) => {
      if (a.id === selectedSessionId) return -1;
      if (b.id === selectedSessionId) return 1;
      return Number(b.lastEventAt || 0) - Number(a.lastEventAt || 0);
    });
}

function activityDisplay(session) {
  const waiting = blockingAttentionForSession(session)[0];
  if (waiting) {
    const text = waiting.kind === "native_approval"
      ? `${providerName(waiting.provider)} 正在请求批准，请回原界面处理`
      : waiting.kind === "approval"
        ? "等待你在 ActRealm 审批"
        : waiting.kind === "question"
          ? "等待你在 ActRealm 回答"
          : "等待你处理";
    return {
      className: "waiting",
      marker: "!",
      text: `${text} · ${turnTiming(session)} · 已等 ${elapsedText(waiting.createdAt)}`,
    };
  }
  const completion = pendingAttentionForSession(session).find((item) => item.kind === "completion");
  if (!isSessionActive(session) && completion) {
    return {
      className: "waiting",
      marker: "✓",
      text: `本轮已完成，等待确认 · ${turnTiming(session)} · 已等 ${elapsedText(completion.createdAt)}`,
    };
  }
  const timing = turnTiming(session);
  if (session.execState === "thinking") {
    return { className: "thinking", marker: "•••", text: `${session.activity || "正在思考"} · ${timing}` };
  }
  if (session.execState === "tool_running") {
    return { className: "tool", marker: "▌", text: `${session.activity || "正在运行工具"} · ${timing}` };
  }
  if (session.execState === "compacting") {
    return { className: "compacting", marker: "◌", text: `${session.activity || "正在压缩记忆"} · ${timing}` };
  }
  if (session.execState === "failed") {
    return { className: "failed", marker: "×", text: `${session.activity || "运行失败"} · ${timing}` };
  }
  if (session.execState === "response_finished") {
    return { className: "idle", marker: "✓", text: `${session.activity || "本轮已完成"} · ${timing}` };
  }
  return { className: "idle", marker: "·", text: `${session.activity || "空闲"} · ${elapsedText(session.lastEventAt)}前` };
}

function updateSessionActivity() {
  for (const [sessionId, ref] of sessionActivityRefs) {
    const session = snapshot.sessions.find((candidate) => candidate.id === sessionId);
    if (!session || !ref.root.isConnected) continue;
    const display = activityDisplay(session);
    ref.root.className = `task-right ${display.className}`;
    ref.root.textContent = display.text;
  }
}

function selectSession(sessionId) {
  if (!sessionId) return;
  selectedSessionId = sessionId;
  renderSessions();
  window.requestAnimationFrame(() => {
    const row = [...ui.sessionList.querySelectorAll(".session-row")]
      .find((candidate) => candidate.dataset.sessionId === sessionId);
    row?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    row?.focus({ preventScroll: true });
  });
}

async function jumpSession(session) {
  if (!session || session.jumpCapability === "unsupported") {
    showToast("当前环境不支持跳转；ActRealm 不会假装已经定位到原对话");
    return;
  }
  try {
    const result = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/jump`, {
      method: "POST",
      body: "{}",
    });
    if (result.success) showToast(result.label || session.jumpLabel || "已打开 Agent");
  } catch (error) {
    const message = error.message === "JUMP_FAILED"
      ? "没有找到原窗口，或 macOS 尚未授予应用控制权限"
      : `跳转失败：${error.message}`;
    showToast(message);
  }
}

async function manageSession(session) {
  if (!session?.canManage) return;
  try {
    await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/manage`, {
      method: "POST",
      body: JSON.stringify({ action: "attach" }),
    });
    showToast("Codex 对话已由 ActRealm app-server Connector 接管");
    await loadSnapshot();
  } catch (error) {
    showToast(`托管连接失败：${error.detail || error.message}`);
  }
}

function activateSession(session) {
  selectSession(session.id);
  void jumpSession(session);
}

async function dismissAttentionForTaskClear(item) {
  if (item.kind === "question" && item.requestId) {
    await api(`/api/v1/questions/${encodeURIComponent(item.requestId)}/answer`, {
      method: "POST",
      body: JSON.stringify({ action: "native" }),
    });
    return;
  }
  await api("/api/v1/commands", {
    method: "POST",
    body: JSON.stringify({
      id: crypto.randomUUID(),
      attentionId: item.id,
      requestId: item.requestId || null,
      action: "dismiss",
    }),
  });
}

async function clearSessionFromList(session) {
  const related = openItems().filter((item) => item.sessionId === session.id);
  const results = await Promise.allSettled(related.map(dismissAttentionForTaskClear));
  const failed = results.filter((result) => result.status === "rejected").length;
  hiddenSessions[session.id] = Number(session.lastEventAt || Date.now());
  localStorage.setItem("actrealm.hiddenSessions", JSON.stringify(hiddenSessions));
  if (selectedSessionId === session.id) selectedSessionId = undefined;
  await loadSnapshot().catch(() => renderSessions());
  if (failed) {
    showToast(`任务已清除；${failed} 项仍需在待处理区或 Agent 原界面处理`);
  } else if (related.length) {
    showToast(`任务已清除，并交还 ${related.length} 项待处理事项`);
  } else {
    showToast("任务已从当前列表清除；收到新事件后会重新出现");
  }
}

function sessionRenderSignature(session) {
  return JSON.stringify({
    session,
    attention: pendingAttentionForSession(session),
    selected: session.id === selectedSessionId,
  });
}

function buildSessionRow(session) {
    const status = sessionStatus(session);
    const row = element("article", `session-row${session.id === selectedSessionId ? " selected" : ""}`);
    row.dataset.sessionId = session.id;
    row.tabIndex = 0;
    const toggle = () => {
      selectedSessionId = selectedSessionId === session.id ? undefined : session.id;
      renderSessions();
    };
    row.addEventListener("click", toggle);
    row.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        toggle();
      }
    });
    const top = element("div", "row-top");
    top.append(providerIcon(session.provider));
    const copy = element("div", "row-copy");
    const title = element("div", "row-title");
    const clientTitle = session.providerTitle || session.title || "等待下一条任务";
    title.append(element("strong", "", clientTitle));
    title.append(element("span", `state-pill ${status.className}`.trim(), status.label));
    const activity = activityDisplay(session);
    const activityNode = element("span", `task-right ${activity.className}`, activity.text);
    sessionActivityRefs.set(session.id, { root: activityNode });
    title.append(activityNode);
    const taskContent = session.providerTitle && session.title && session.providerTitle !== session.title
      ? element("div", "session-question", session.title)
      : undefined;
    copy.append(title);
    if (taskContent) copy.append(taskContent);
    copy.append(element("div", "session-meta", `${providerName(session.provider)} · ${session.model || "模型未知"}`));
    appendUsageStrip(copy, session);
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
      progress.append(label, track, element("span", "", `${session.activeSubagents || 0} 个子 Agent 正在运行`));
      copy.append(progress);
    }
    const clear = element("button", "session-clear", "清除");
    clear.type = "button";
    clear.title = "从当前任务列表隐藏；收到新事件后会重新出现";
    clear.addEventListener("click", (event) => {
      event.stopPropagation();
      clear.disabled = true;
      void clearSessionFromList(session);
    });
    copy.append(clear);

    if (session.id === selectedSessionId) {
      const details = element("div", "task-expanded");
      const plan = Number(session.planTotal || 0) > 0 ? `${session.planDone || 0}/${session.planTotal}（进行中）` : "未提供";
      const pairs = [
        ["工作区", session.environment || session.project || `${providerName(session.provider)} 客户端`],
        ["本轮上下文", contextUsageText(session)],
        ["计划", plan],
        ["会话累计 Token", session.tokenTotal == null ? "—" : compactCount(session.tokenTotal)],
        ["本轮 Token", session.lastTurnTokens == null ? "—" : compactCount(session.lastTurnTokens)],
        ["估算 API 价", estimatedCostText(session) || "—"],
      ];
      for (const [label, value] of pairs) {
        const pair = element("div", "task-detail-pair");
        pair.append(element("span", "", label), element("strong", "", value));
        details.append(pair);
      }
      const waiting = openItems().find((item) => item.sessionId === session.id);
      if (waiting) {
        const locate = element("button", "task-locate", "查看待处理事项");
        locate.type = "button";
        locate.addEventListener("click", (event) => {
          event.stopPropagation();
          currentAttention = openItems().findIndex((item) => item.id === waiting.id);
          renderAttention();
          document.querySelector(".outbox-panel")?.scrollIntoView({ behavior: "smooth", block: "nearest" });
        });
        details.append(locate);
      }
      copy.append(details);
    }
    top.append(copy);
    row.append(top);
    return row;
}

function renderSessions() {
  const sessions = visibleSessions();
  if (selectedSessionId && !sessions.some((session) => session.id === selectedSessionId)) {
    selectedSessionId = undefined;
  }
  const waitingCount = sessions.filter((session) => sessionStatus(session).className === "waiting").length;
  const runningCount = sessions.filter((session) => isSessionActive(session) && sessionStatus(session).className !== "waiting").length;
  const finishedCount = sessions.filter((session) => ["idle", "response_finished"].includes(session.execState)).length;
  ui.sessionCount.textContent = `${sessions.length} 个任务 · ${waitingCount} 等你 · ${runningCount} 在跑 · ${finishedCount} 刚完成`;
  if (!sessions.length) {
    selectedSessionId = undefined;
    sessionActivityRefs.clear();
    sessionRenderSignatures.clear();
    const emptyKind = isFirstRun() ? "onboarding" : "regular";
    const currentEmpty = ui.sessionList.querySelector(".session-empty");
    if (!currentEmpty || currentEmpty.dataset.emptyKind !== emptyKind) {
      const empty = isFirstRun()
        ? onboardingTaskEmpty()
        : emptyState("✓", "当前没有活跃任务", "这里只保留运行中、待处理或最近 30 分钟内的任务。");
      empty.classList.add("session-empty");
      empty.dataset.emptyKind = emptyKind;
      ui.sessionList.replaceChildren(empty);
    }
    return;
  }

  const scrollTop = ui.sessionList.scrollTop;
  const focusedSessionId = document.activeElement?.closest?.(".session-row")?.dataset.sessionId;
  const existingRows = new Map(
    [...ui.sessionList.querySelectorAll(".session-row[data-session-id]")]
      .map((row) => [row.dataset.sessionId, row]),
  );
  const nextIds = new Set(sessions.map((session) => session.id));
  const desiredRows = [];
  for (const session of sessions) {
    const signature = sessionRenderSignature(session);
    const existing = existingRows.get(session.id);
    let row = existing;
    if (!existing || sessionRenderSignatures.get(session.id) !== signature) {
      sessionActivityRefs.delete(session.id);
      row = buildSessionRow(session);
      if (existing) existing.replaceWith(row);
    }
    sessionRenderSignatures.set(session.id, signature);
    desiredRows.push(row);
  }
  for (let index = 0; index < desiredRows.length; index += 1) {
    const row = desiredRows[index];
    const current = ui.sessionList.children[index];
    if (current !== row) ui.sessionList.insertBefore(row, current || null);
  }
  for (const child of [...ui.sessionList.children]) {
    const sessionId = child.dataset?.sessionId;
    if (!sessionId || !nextIds.has(sessionId)) child.remove();
  }
  for (const sessionId of [...sessionRenderSignatures.keys()]) {
    if (!nextIds.has(sessionId)) {
      sessionRenderSignatures.delete(sessionId);
      sessionActivityRefs.delete(sessionId);
    }
  }
  ui.sessionList.scrollTop = scrollTop;
  if (focusedSessionId) {
    ui.sessionList.querySelector(`[data-session-id="${CSS.escape(focusedSessionId)}"]`)?.focus({ preventScroll: true });
  }
}

function quotaDurationLabel(minutes, fallback) {
  const value = Number(minutes || 0);
  if (value > 0 && value % 43200 === 0) return `${value / 43200} 个月`;
  if (value > 0 && value % 10080 === 0) return `${value / 10080} 周`;
  if (value > 0 && value % 1440 === 0) return `${value / 1440} 天`;
  if (value > 0 && value % 60 === 0) return `${value / 60} 小时`;
  if (value > 0) return `${value} 分钟`;
  if (fallback === "5h") return "5 小时";
  if (fallback === "7d") return "7 天";
  return fallback && fallback !== "unknown" ? fallback.replaceAll("_", " ") : "额度";
}

function quotaWindowLabel(quota) {
  const name = quota.limitName || quotaDurationLabel(quota.windowMinutes, quota.window);
  return `${providerName(quota.provider)} · ${name}`;
}

function quotaSlots() {
  return [...(snapshot.quota || [])].sort((a, b) => {
    const providerOrder = { claude: 0, codex: 1 };
    return (providerOrder[a.provider] ?? 9) - (providerOrder[b.provider] ?? 9)
      || Number(a.windowMinutes || Number.MAX_SAFE_INTEGER) - Number(b.windowMinutes || Number.MAX_SAFE_INTEGER)
      || String(a.window).localeCompare(String(b.window));
  });
}

function quotaRenderSignature() {
  return JSON.stringify(quotaSlots());
}

function updateQuotaTimes() {
  for (const node of ui.quotaList.querySelectorAll("[data-quota-captured-at]")) {
    const minutes = Math.max(0, Math.floor((Date.now() - Number(node.dataset.quotaCapturedAt)) / 60000));
    node.textContent = minutes > 0 ? `${minutes} 分钟前更新` : "刚刚更新";
  }
  for (const node of ui.quotaList.querySelectorAll("[data-quota-resets-at]")) {
    const reset = new Date(Number(node.dataset.quotaResetsAt));
    node.textContent = reset.getTime() <= Date.now()
      ? "已到重置时间，等待同步"
      : `${reset.toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit" })} 重置`;
  }
}

function renderQuota() {
  lastQuotaRenderSignature = quotaRenderSignature();
  ui.quotaList.replaceChildren();
  if (isFirstRun()) {
    ui.quotaSyncTime.textContent = "最近同步 · 等待 Agent 接入";
    ui.quotaList.append(onboardingQuotaState());
    return;
  }
  const slots = quotaSlots();
  const latestCapture = Math.max(0, ...slots.map((quota) => Number(quota.capturedAt || 0)));
  ui.quotaSyncTime.textContent = latestCapture
    ? `最近同步 · ${new Date(latestCapture).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`
    : "最近同步 · 等待额度来源";
  if (!slots.length) {
    ui.quotaList.append(emptyState("—", "暂时没有额度数据", "完成一次 Agent 对话后会在这里同步可验证额度。"));
    return;
  }
  for (const quota of slots) {
    const label = quotaWindowLabel(quota);
    const hasLastValue = ["available", "stale"].includes(quota.status)
      && typeof quota.usedPct === "number"
      && typeof quota.remainingPct === "number";
    if (!hasLastValue) {
      const unavailable = element("article", "quota-unavailable");
      const unavailableTitle = element("div", "row-title");
      unavailableTitle.append(providerIcon(quota.provider), element("strong", "", label));
      unavailable.append(unavailableTitle);
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
    const row = element("article", `quota-row${quota.status === "stale" ? " stale" : ""}`);
    const title = element("div", "row-title");
    title.append(providerIcon(quota.provider), element("strong", "", label));
    title.append(element("span", "section-meta", `剩余 ${Math.round(quota.remainingPct)}%`));
    row.append(title);
    const track = element("div", "quota-track");
    const fill = element("div", "quota-fill");
    fill.style.width = `${Math.max(0, Math.min(100, quota.remainingPct))}%`;
    fill.classList.add(quota.remainingPct >= 50 ? "healthy" : quota.remainingPct >= 20 ? "warning" : "critical");
    track.append(fill);
    row.append(track);
    const meta = element("div", "quota-meta");
    if (quota.status === "stale") meta.append(element("span", "quota-stale", "保留上次有效值"));
    const sourceLabel = {
      oauth_usage: "OAuth 自动同步",
      statusline: "Claude 对话同步",
      rollout_experimental: "本机 Session 同步",
    }[quota.source];
    if (sourceLabel) meta.append(element("span", "quota-source", sourceLabel));
    if (quota.planType) meta.append(element("span", "", quota.planType));
    if (quota.resetsAt) {
      const reset = new Date(Number(quota.resetsAt) * 1000);
      const resetNode = element("span", "");
      resetNode.dataset.quotaResetsAt = String(reset.getTime());
      meta.append(resetNode);
    }
    if (quota.capturedAt) {
      const capturedNode = element("span", "");
      capturedNode.dataset.quotaCapturedAt = String(Number(quota.capturedAt));
      meta.append(capturedNode);
    }
    row.append(meta);
    ui.quotaList.append(row);
  }
  updateQuotaTimes();
}

function notificationRule(item) {
  const kind = item.kind === "native_approval" ? "approval" : item.kind;
  return settingsState.notificationRules?.[kind] === "ignore" ? "ignore" : "list";
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
  ui.notificationKind.textContent = `${providerName(item.provider)} · ${attentionContext(item).notification}`;
  ui.notificationTitle.textContent = attentionTitle(item);
  ui.notificationBanner.hidden = false;
  playNotificationSound();
  void recordUiMetric("banner_shown");
}

function processNotifications(nextSnapshot) {
  const nextItems = (nextSnapshot.attention || []).filter((item) => ["open", "committing", "decision_sent"].includes(item.state));
  if (notificationsPrimed) {
    const item = nextItems.find((candidate) => !knownAttentionIds.has(candidate.id));
    if (item && notificationRule(item) === "list" && !isProviderMuted(item.provider)) playNotificationSound();
  }
  knownAttentionIds = new Set(nextItems.map((item) => item.id));
}

function render(nextSnapshot) {
  const previousOpenIds = new Set(openItems().map((item) => item.id));
  const nextOpenIds = new Set(
    (nextSnapshot.attention || [])
      .filter((item) => ["open", "committing", "decision_sent"].includes(item.state))
      .map((item) => item.id),
  );
  const attentionWasResolved = [...previousOpenIds].some((id) => !nextOpenIds.has(id));
  processNotifications(nextSnapshot);
  snapshot = nextSnapshot;
  if (attentionWasResolved && !attentionExitTimer) {
    ui.attentionList.querySelector(".attention-card")?.classList.add("attention-card-leaving");
    attentionExitTimer = window.setTimeout(() => {
      attentionExitTimer = undefined;
      renderAttention();
    }, 180);
  } else if (!attentionExitTimer && attentionRenderSignature() !== lastAttentionRenderSignature) {
    renderAttention();
  } else {
    updateAttentionTimes();
  }
  renderSessions();
  if (quotaRenderSignature() !== lastQuotaRenderSignature) renderQuota();
  else updateQuotaTimes();
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
  if (csrfToken && options.method && options.method !== "GET") headers.set("x-actrealm-csrf", csrfToken);
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

async function bootstrapWithToken(token) {
  const response = await api("/api/v1/bootstrap", { method: "POST", body: JSON.stringify({ token }) });
  csrfToken = response.csrfToken;
  sessionStorage.setItem("actrealm.csrf", csrfToken);
  return true;
}

async function bootstrap() {
  const token = new URLSearchParams(location.hash.slice(1)).get("bootstrap");
  if (!token) return false;
  await bootstrapWithToken(token);
  history.replaceState(null, "", `${location.pathname}${location.search}`);
  return true;
}

async function loadSnapshot() {
  const previousEventCount = Number(snapshot.stats?.eventCount || 0);
  try {
    const nextSnapshot = await api("/api/v1/snapshot");
    render(nextSnapshot);
    setConnected(true);
    lastSnapshotAt = Date.now();
    if (Number(nextSnapshot.stats?.eventCount || 0) !== previousEventCount) scheduleSetupRefresh();
  } catch (error) {
    if (String(error.message) === "UNAUTHORIZED" && await bootstrap()) {
      const nextSnapshot = await api("/api/v1/snapshot");
      render(nextSnapshot);
      setConnected(true);
      lastSnapshotAt = Date.now();
      if (Number(nextSnapshot.stats?.eventCount || 0) !== previousEventCount) scheduleSetupRefresh();
      return;
    }
    throw error;
  }
}

function setConnected(connected) {
  const changed = runtimeConnected !== connected;
  runtimeConnected = connected;
  document.body.classList.toggle("disconnected", !connected);
  ui.runtimeState.classList.toggle("online", connected);
  ui.runtimeLabel.textContent = connected ? "Live · 本地" : "正在重连";
  ui.runtimeFooterLabel.textContent = connected ? "Runtime · 本机在线" : "Runtime · 正在重连";
  ui.runtimeSettingsLabel.textContent = connected ? "本机 Runtime 在线" : "本机 Runtime 正在重连";
  ui.setupRefresh.disabled = !connected;
  ui.offlineBanner.hidden = connected;
  if (changed && setupLoaded && !ui.setupOverlay.hidden) renderSetup();
}

function scheduleSetupRefresh() {
  window.clearTimeout(setupRefreshTimer);
  setupRefreshTimer = window.setTimeout(() => {
    setupRefreshTimer = undefined;
    void loadSetup();
  }, 500);
}

function connectSocket() {
  if (!csrfToken || restartInProgress) return;
  if (socket && [WebSocket.CONNECTING, WebSocket.OPEN].includes(socket.readyState)) return;
  window.clearTimeout(reconnectTimer);
  reconnectTimer = undefined;
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const currentSocket = new WebSocket(`${scheme}://${location.host}/api/v1/ws?csrf=${encodeURIComponent(csrfToken)}`);
  socket = currentSocket;
  currentSocket.addEventListener("open", () => {
    if (socket !== currentSocket) return;
    reconnectDelay = 500;
    lastSocketFrameAt = Date.now();
  });
  currentSocket.addEventListener("message", (event) => {
    if (socket !== currentSocket) return;
    try {
      const frame = JSON.parse(event.data);
      lastSocketFrameAt = Date.now();
      setConnected(true);
      if (frame.type === "heartbeat") return;
      if (frame.type === "snapshot") {
        const previousEventCount = Number(snapshot.stats?.eventCount || 0);
        render(frame.snapshot);
        lastSnapshotAt = Date.now();
        if (Number(snapshot.stats?.eventCount || 0) !== previousEventCount) scheduleSetupRefresh();
      }
    } catch (_) {
      showToast("Runtime 返回了无法识别的消息");
    }
  });
  currentSocket.addEventListener("close", () => {
    if (socket !== currentSocket) return;
    socket = undefined;
    setConnected(false);
    if (restartInProgress) return;
    reconnectTimer = window.setTimeout(connectSocket, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 10000);
  });
  currentSocket.addEventListener("error", () => currentSocket.close());
}

async function refreshSnapshotFallback() {
  if (fallbackSnapshotInFlight || !csrfToken) return;
  fallbackSnapshotInFlight = true;
  try {
    await loadSnapshot();
  } catch (_) {
    // The WebSocket reconnect path owns the visible connection state.
  } finally {
    fallbackSnapshotInFlight = false;
  }
}

function maintainLiveConnection() {
  if (document.visibilityState !== "visible" || restartInProgress) return;
  const now = Date.now();
  if (socket?.readyState === WebSocket.OPEN && now - lastSocketFrameAt > SOCKET_STALE_AFTER_MS) {
    socket.close();
    return;
  }
  if (now - lastSnapshotAt > SNAPSHOT_FALLBACK_AFTER_MS) void refreshSnapshotFallback();
}

async function sendAction(item, action) {
  const id = crypto.randomUUID();
  if (action === "pass_through") selectSession(item.sessionId);
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

function chooseReminder(rule, value) {
  const control = {
    approval: ui.notifyApproval,
    question: ui.notifyQuestion,
    error: ui.notifyError,
    completion: ui.notifyCompletion,
  }[rule];
  if (!control) return;
  control.value = value;
  settingsState.notificationRules = { ...(settingsState.notificationRules || {}), [rule]: value };
  renderSettings();
  saveSettings();
}

function chooseRetention(value) {
  ui.retentionDays.value = String(value);
  settingsState.retentionDays = Number(value);
  renderSettings();
  saveSettings();
}

function monitorItem(label, value, status = "") {
  const item = element("div", "monitor-item");
  item.append(element("span", "", label), element("strong", status, value));
  return item;
}

function renderRuntimeMonitor(status) {
  const hookStatus = {
    ready: ["Hook 通道正常", "ready"],
    insecure: ["Socket 权限异常", "warning"],
    missing: ["Hook 通道缺失", "warning"],
    invalid: ["Socket 类型异常", "warning"],
    unavailable: ["Hook 通道不可用", "warning"],
  }[status.hook?.status] || ["Hook 状态未知", "warning"];
  const lastHook = status.hook?.lastEventAt
    ? `${elapsedText(status.hook.lastEventAt)}前`
    : "尚无真实事件";
  const restartResult = status.restart?.lastResult === "recovered"
    ? `已恢复 · ${status.restart.count} 次`
    : "本次启动未重启";
  ui.runtimeMonitorGrid.replaceChildren(
    monitorItem("Runtime", `PID ${status.pid} · ${status.version}`, "ready"),
    monitorItem("运行时间", elapsedText(status.startedAt)),
    monitorItem("本地 API", status.api?.status === "ready" ? "在线" : "异常", status.api?.status === "ready" ? "ready" : "warning"),
    monitorItem("WebSocket", `${status.websocket?.connections || 0} 个连接`, "ready"),
    monitorItem(status.hook?.name || "bridge.sock", hookStatus[0], hookStatus[1]),
    monitorItem("最近 Hook", lastHook),
    monitorItem("任务 / 待处理", `${status.sessions?.active || 0} 活跃 · ${status.attention?.pending || 0} 待处理`),
    monitorItem("SQLite / 重启", `${status.storage?.eventCount || 0} 事件 · ${restartResult}`, "ready"),
  );
}

async function loadRuntimeMonitor() {
  if (runtimeMonitorInFlight || ui.runtimeMonitorDetails.hidden) return;
  runtimeMonitorInFlight = true;
  ui.runtimeMonitorRefresh.disabled = true;
  try {
    renderRuntimeMonitor(await api("/api/v1/runtime/status"));
    lastRuntimeMonitorAt = Date.now();
  } catch (error) {
    ui.runtimeMonitorGrid.replaceChildren(element("span", "monitor-loading", `监控读取失败：${error.message}`));
  } finally {
    runtimeMonitorInFlight = false;
    ui.runtimeMonitorRefresh.disabled = false;
  }
}

async function readPublicHealth() {
  const response = await fetch("/api/v1/health", { cache: "no-store", credentials: "same-origin" });
  if (!response.ok) throw new Error(`HEALTH_${response.status}`);
  return response.json();
}

async function waitForRuntimeRestart(previousInstanceId, restartToken) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    let health;
    try {
      health = await readPublicHealth();
    } catch (_) {
      // The old listener must disappear before the same port can be rebound.
    }
    if (health?.instanceId && health.instanceId !== previousInstanceId) {
      csrfToken = undefined;
      sessionStorage.removeItem("actrealm.csrf");
      await bootstrapWithToken(restartToken);
      await loadSnapshot();
      await loadSetup();
      await loadSettings();
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 250));
  }
  throw new Error("RESTART_TIMEOUT");
}

async function restartRuntime() {
  if (restartInProgress) return;
  const waiting = openItems().filter((item) => item.kind === "approval" || item.kind === "native_approval").length;
  if (waiting > 0 && !window.confirm(`当前有 ${waiting} 个请求正在等待。重启会安全放行这些请求并重新连接 Agent。`)) return;
  restartInProgress = true;
  ui.runtimeRestart.disabled = true;
  ui.runtimeRestart.textContent = "正在保存状态…";
  const restartToken = crypto.randomUUID();
  try {
    const previous = await readPublicHealth();
    let accepted;
    try {
      accepted = await api("/api/v1/runtime/restart", {
        method: "POST",
        body: JSON.stringify({ restartToken }),
      });
    } catch (error) {
      // The Runtime may close the old listener after accepting the command but
      // before the HTTP response reaches the browser. The one-time token makes
      // it safe to continue recovery; a rejected command still times out.
      if (!(error instanceof TypeError)) throw error;
      accepted = { previousInstanceId: previous.instanceId };
    }
    const previousInstanceId = accepted.previousInstanceId || previous.instanceId;
    window.clearTimeout(reconnectTimer);
    reconnectTimer = undefined;
    if (socket) {
      const previousSocket = socket;
      socket = undefined;
      previousSocket.close();
    }
    csrfToken = undefined;
    sessionStorage.removeItem("actrealm.csrf");
    setConnected(false);
    ui.runtimeLabel.textContent = "正在重启";
    ui.runtimeFooterLabel.textContent = "Runtime · 正在恢复";
    ui.runtimeSettingsLabel.textContent = "本机 Runtime 正在重启";
    ui.runtimeRestart.textContent = "正在恢复连接…";
    await waitForRuntimeRestart(previousInstanceId, restartToken);
    restartInProgress = false;
    reconnectDelay = 500;
    connectSocket();
    setConnected(true);
    await loadRuntimeMonitor();
    showToast("Runtime 已重启 · Hook 通道与任务状态已恢复");
  } catch (error) {
    restartInProgress = false;
    setConnected(false);
    showToast(error.message === "RESTART_TIMEOUT"
      ? "Runtime 未能自动恢复，请在终端重新运行 serve --open"
      : `Runtime 重启失败：${error.message}`);
  } finally {
    ui.runtimeRestart.disabled = false;
    ui.runtimeRestart.textContent = "重启 Runtime";
  }
}

ui.undoButton.addEventListener("click", () => {
  if (undoCommandId) undoCommand(undoCommandId);
});
ui.setupTrigger.addEventListener("click", openSetup);
ui.agentStatusTrigger.addEventListener("click", openSetup);
ui.setupClose.addEventListener("click", closeSetup);
ui.setupRefresh.addEventListener("click", loadSetup);
ui.settingsTrigger.addEventListener("click", openSettings);
ui.settingsClose.addEventListener("click", closeSettings);
ui.settingsOverlay.addEventListener("click", (event) => {
  if (event.target === ui.settingsOverlay) closeSettings();
});
ui.displayProfile.addEventListener("change", () => {
  settingsState.displayProfile = ui.displayProfile.value;
  settingsState.taskCardFields = [...(DISPLAY_PRESETS[ui.displayProfile.value] || DISPLAY_PRESETS.detailed)];
  renderFieldSelector();
  saveSettings();
});
ui.taskCardFields.addEventListener("change", () => {
  settingsState.taskCardFields = [...ui.taskCardFields.querySelectorAll("input:checked")].map((input) => input.value);
  saveSettings();
});
ui.sessionDetailClose.addEventListener("click", closeSessionDetail);
ui.sessionDetailOverlay.addEventListener("click", (event) => {
  if (event.target === ui.sessionDetailOverlay) closeSessionDetail();
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
ui.clearData.addEventListener("click", openClearConfirmation);
ui.wipeConfirm.addEventListener("click", clearLocalData);
ui.wipeCancel.addEventListener("click", cancelClearConfirmation);
ui.wipeConfirmationInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") clearLocalData();
});
for (const row of ui.reminderRows) {
  for (const button of row.querySelectorAll("button[data-value]")) {
    button.addEventListener("click", () => chooseReminder(row.dataset.rule, button.dataset.value));
  }
}
for (const button of ui.retentionOptions) {
  button.addEventListener("click", () => chooseRetention(Number(button.dataset.value)));
}
ui.runtimeMonitor.addEventListener("click", () => {
  ui.runtimeMonitorDetails.hidden = !ui.runtimeMonitorDetails.hidden;
  ui.runtimeMonitor.textContent = ui.runtimeMonitorDetails.hidden ? "查看监控" : "收起监控";
  if (!ui.runtimeMonitorDetails.hidden) void loadRuntimeMonitor();
});
ui.runtimeMonitorRefresh.addEventListener("click", loadRuntimeMonitor);
ui.runtimeRestart.addEventListener("click", restartRuntime);
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
  if (!ui.sessionDetailOverlay.hidden) {
    closeSessionDetail();
    return;
  }
  if (!ui.setupOverlay.hidden) closeSetup();
  if (!ui.settingsOverlay.hidden) closeSettings();
});

function updateLiveTimes() {
  updateClock();
  updateSessionActivity();
  updateAttentionTimes();
  updateQuotaTimes();
  if (!ui.settingsOverlay.hidden
      && !ui.runtimeMonitorDetails.hidden
      && Date.now() - lastRuntimeMonitorAt >= 2000) {
    void loadRuntimeMonitor();
  }
}

function resumeLiveView() {
  updateLiveTimes();
  maintainLiveConnection();
  if (!socket || socket.readyState === WebSocket.CLOSED) connectSocket();
  if (!setupLoaded || Date.now() - lastSetupAt >= SETUP_FOCUS_REFRESH_AFTER_MS) void loadSetup();
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") resumeLiveView();
});
window.addEventListener("focus", resumeLiveView);
window.addEventListener("pageshow", resumeLiveView);
updateLiveTimes();
window.setInterval(updateLiveTimes, 1000);
window.setInterval(maintainLiveConnection, 5000);

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
    ui.attentionList.replaceChildren(emptyState("!", "无法连接本地 Runtime", "请从 actrealm serve 输出的一次性地址打开控制面板。"));
    showToast(`连接失败：${error.message}`);
  } finally {
    document.body.classList.remove("app-booting");
  }
})();
