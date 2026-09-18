"use strict";

const I18N = window.ActRealmI18n;
const agentState = window.ActRealmAgentState;
const agentDetail = window.ActRealmAgentDetail;
const tr = (text) => I18N.tr(text);
const currentLocale = () => I18N.resolvedLocale();

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
  settingsSaveFeedback: document.querySelector("#settings-save-feedback"),
  settingsSaveMessage: document.querySelector("#settings-save-message"),
  settingsSaveRetry: document.querySelector("#settings-save-retry"),
  notifyApproval: document.querySelector("#notify-approval"),
  notifyQuestion: document.querySelector("#notify-question"),
  notifyError: document.querySelector("#notify-error"),
  notifyCompletion: document.querySelector("#notify-completion"),
  soundEnabled: document.querySelector("#sound-enabled"),
  muteClaude: document.querySelector("#mute-claude"),
  muteCodex: document.querySelector("#mute-codex"),
  codexEnhanced: document.querySelector("#codex-enhanced"),
  codexConnectorStatus: document.querySelector("#codex-connector-status"),
  completionHideMode: document.querySelector("#completion-hide-mode"),
  completionAutoHideRow: document.querySelector("#completion-auto-hide-row"),
  completionAutoHideMinutes: document.querySelector("#completion-auto-hide-minutes"),
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
  runtimeActionFeedback: document.querySelector("#runtime-action-feedback"),
  retentionOptions: [...document.querySelectorAll(".retention-options button")],
  quotaDisplayOptions: [...document.querySelectorAll(".quota-display-options button")],
  languageSelect: document.querySelector("#language-select"),
  wipeConfirmation: document.querySelector("#wipe-confirmation"),
  wipeConfirmationInput: document.querySelector("#wipe-confirmation-input"),
  wipeConfirm: document.querySelector("#wipe-confirm"),
  wipeCancel: document.querySelector("#wipe-cancel"),
  backupSummary: document.querySelector("#backup-summary"),
  clearBackups: document.querySelector("#clear-backups"),
  backupWipeConfirmation: document.querySelector("#backup-wipe-confirmation"),
  backupWipeConfirmationInput: document.querySelector("#backup-wipe-confirmation-input"),
  backupWipeConfirm: document.querySelector("#backup-wipe-confirm"),
  backupWipeCancel: document.querySelector("#backup-wipe-cancel"),
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
let currentAttentionID;
let socket;
let runtimeConnected = false;
let reconnectDelay = 500;
let undoCommandId;
let toastTimer;
let toastPriority = 0;
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
  displayFieldsVersion: 5,
  taskCardFields: [...agentDetail.presets.detailed],
  quotaDisplayMode: "standard",
  tokenUsageDisplayMode: "standard",
  tokenUsageComponentsVisible: true,
  tokenUsageHeatmapVisible: true,
  tokenUsageCostVisible: true,
  tokenUsageObservedTimeVisible: true,
  tokenUsageExecutionTimeVisible: true,
  tokenUsageUnitStyle: "automatic",
  tokenUsageTaskProjectVisible: true,
  tokenUsageBurnRateVisible: true,
  tokenUsageAnomalyVisible: true,
  tokenThresholdNotificationsEnabled: false,
  tokenThresholdTokensPerMinute: 250000,
  completionTaskHideMode: "afterConfirmation",
  completionAutoHideMinutes: 30,
};
let displayCatalog = [];
let claudeBridge = { status: "not_installed" };
let backupSummary = { count: 0, totalBytes: 0 };
let settingsBusy = false;
let failedSettingsDraft;
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
let socketTicketInFlight = false;
let restartInProgress = false;
let runtimeMonitorInFlight = false;
let lastRuntimeMonitorAt = 0;
let hiddenSessions = JSON.parse(localStorage.getItem("actrealm.hiddenSessions") || "{}");
const SESSION_VISIBLE_FOR_MS = 30 * 60 * 1000;
const RUNTIME_MESSAGES_ZH = {
  "interaction.agent_question.title": "Agent 正在询问",
  "quota.reason.agent_refresh_failed": "Agent 额度刷新失败，显示上次记录。",
  "quota.reason.agent_unavailable": "Agent 暂未提供账户额度信息。",
  "quota.window.claude_weekly": "总周额度",
  "quota.window.scoped_weeks": "{name} · {count} 周",
  "quota.window.scoped": "{name} 额度",
  "quota.reason.cache_stale": "这是历史额度，正在等待 Provider 更新；不能作为当前剩余额度。",
  "session.activity.idle": "等待新任务",
  "session.activity.ended": "会话已结束",
  "session.activity.thinking": "正在思考",
  "session.activity.tool_running": "正在运行 {tool}",
  "session.activity.awaiting_approval": "等待你批准",
  "session.activity.awaiting_answer": "等待你回答",
  "session.activity.compacting": "正在压缩记忆",
  "session.activity.completed": "本轮已完成",
  "session.activity.interrupted": "本轮已中断",
  "session.activity.failed": "运行失败",
  "session.activity.plan_progress": "计划进度 {done}/{total}",
  "session.activity.subagents_running": "{count} 个子 Agent 正在运行",
  "session.activity.background_tasks_running": "{count} 个后台任务仍在运行",
  "session.activity.permission_denied": "操作已在 Agent 中拒绝",
  "session.activity.waiting_for_provider_event": "等待 Agent 后续事件",
  "session.activity.unknown_event": "事件不识别，Provider 版本可能不兼容",
  "attention.approval.title": "等待批准",
  "attention.native_approval.title": "请在 {provider} 中批准",
  "attention.question.title": "{provider} 正在询问",
  "attention.error.title": "Agent 运行失败",
  "attention.interrupted.title": "Agent 本轮已中断",
  "attention.completion.title": "任务已完成，等待确认",
  "attention.approval.detail": "请在原对话中核对操作内容和影响。",
  "attention.native_approval.detail": "请在 {provider} 中查看并处理此请求。",
  "attention.question.detail": "可直接在 ActRealm 回答；答案不会写入本地历史。",
  "attention.risk.high_impact": "已识别到高影响操作",
  "attention.risk.irreversible": "提交后动作本身不可撤销",
  "attention.risk.compound_syntax": "命令包含组合语法",
  "attention.risk.read_only_intent": "只读意图；规则提示不构成安全保证",
  "attention.risk.undo_window": "批准决定可在 3 秒内撤回",
  "attention.risk.side_effects": "可能执行项目代码或产生副作用",
  "attention.risk.unknown_impact": "此操作的影响未知",
  "attention.risk.review_original": "建议查看原窗口",
  "interaction.claude_question.title": "Claude 正在询问",
  "interaction.claude_elicitation.title": "Claude 需要补充信息",
  "interaction.codex_user_input.title": "Codex 正在询问",
  "jump.exact_conversation": "精确打开对话",
  "jump.terminal": "打开对应终端",
  "jump.app_only": "只能打开应用",
  "jump.unsupported": "当前环境不支持跳转",
  "quota.window.months": "{count} 个月",
  "quota.window.weeks": "{count} 周",
  "quota.window.days": "{count} 天",
  "quota.window.hours": "{count} 小时",
  "quota.window.minutes": "{count} 分钟",
  "quota.window.current_week": "本周",
  "quota.window.extra_usage": "额外用量",
  "quota.reason.cache_missing": "额度缓存不存在，请开启 Claude 额度桥并完成一次对话。",
  "quota.reason.cache_unreadable": "额度缓存不可读：{error}",
  "quota.reason.cache_incompatible": "额度缓存版本不兼容。",
  "quota.reason.cache_invalid": "额度缓存解析失败。",
  "quota.reason.cache_from_future": "额度缓存时间晚于本机时间。",
  "quota.reason.no_valid_window": "没有找到可验证的额度窗口。",
  "quota.reason.codex_rollout_missing": "未找到 Codex rollout 文件。",
  "quota.reason.codex_window_missing": "Codex rollout 中没有可验证的额度窗口。",
  "quota.reason.claude_refresh_failed": "Claude 额度刷新失败，当前显示的是上次成功获取的数据。",
  "quota.reason.codex_refresh_failed": "Codex 额度刷新失败，当前显示的是上次成功获取的数据。",
};

const API_ERRORS_ZH = {
  CLAUDE_SIGN_IN_REQUIRED: "请先登录 Claude Code，再刷新额度；无需发送对话。",
  CLAUDE_AUTH_REFRESH_FAILED: "Claude 凭据自动续期失败，请检查登录状态或重试。",
  CLAUDE_QUOTA_RATE_LIMITED: "Claude 额度请求被限流，将稍后自动重试。",

  ARTIFACT_REVEAL_FAILED: "访达未能定位该文件，请重试",
  ARTIFACT_UNAVAILABLE: "关联文件已不可用，或已不属于当前结果",
  ANSWER_FAILED: "回答未能发送给 Agent",
  AUTH_PERSIST_FAILED: "伴生应用授权无法保存",
  AUTH_UNAVAILABLE: "Runtime 身份验证暂不可用",
  BACKUP_CLEAR_FAILED: "配置备份未能安全清除",
  BACKUP_DELETE_CONFIRMATION_REQUIRED: "需要输入 DELETE BACKUPS 才能清除配置备份",
  CLAUDE_BRIDGE_CHANGE_FAILED: "Claude 额度桥更新失败",
  CLAUDE_OAUTH_DISABLED: "Claude 官方 OAuth 额度接口未启用",
  CLAUDE_QUOTA_REFRESH_FAILED: "Claude 额度刷新失败",
  CLEAR_FAILED: "本地数据清除失败",
  CODEX_REINSTALL_FAILED: "Codex Hook 重新安装失败",
  COMMAND_MISMATCH: "命令与当前请求不匹配",
  COMMIT_TOO_EARLY: "决定仍在撤回窗口内",
  COMPANION_NOT_FOUND: "没有找到对应的伴生应用连接",
  COMPANION_SCOPE_REQUIRED: "当前伴生应用没有这项操作权限",
  COMPANION_UNAUTHORIZED: "伴生应用连接无效或已撤销",
  CONNECTOR_ATTACH_FAILED: "Connector 连接失败",
  CURRENT_TURN_REQUIRED: "向前加载任务时间线时必须限定当前阶段",
  DELETE_CONFIRMATION_REQUIRED: "需要输入 DELETE 才能清除数据",
  EXPORT_FAILED: "本地数据导出失败",
  INVALID_ACTION: "当前操作无效",
  INVALID_ANSWER: "回答内容无效",
  INVALID_BOOTSTRAP: "启动凭据无效或已过期",
  INVALID_CLIENT_NAME: "伴生应用名称无效",
  INVALID_COMMAND_ID: "命令标识无效",
  INVALID_COMPANION_ID: "伴生应用标识无效",
  INVALID_HOST: "访问地址不是受信任的本机地址",
  INVALID_HISTORY_LIMIT: "历史任务读取数量无效",
  INVALID_ORIGIN: "请求来源不受信任",
  INVALID_PAIRING_CODE: "配对码无效或已经使用",
  INVALID_RESTART_TOKEN: "Runtime 重启凭据无效",
  INVALID_SESSION_ID: "任务标识无效",
  INVALID_SETTINGS: "设置内容无效",
  CHECKPOINT_NOT_FOUND: "没有找到对应 Checkpoint",
  CHECKPOINT_INVALID: "Checkpoint 请求无效",
  CHECKPOINT_GIT_FAILED: "无法安全创建或应用 Git Checkpoint",
  CHECKPOINT_PREFLIGHT_FAILED: "Checkpoint 恢复预检未通过",
  INVALID_TIMELINE_CURSOR: "任务时间线只能使用一个游标",
  JUMP_FAILED: "没有找到原窗口，或 macOS 尚未授予应用控制权限",
  JUMP_UNSUPPORTED: "当前环境不支持跳转",
  MANAGED_CONNECTOR_UNSUPPORTED: "当前会话不支持托管 Connector",
  METRIC_RECORD_FAILED: "本地统计记录失败",
  MISSING_REQUEST_ID: "当前请求没有可回复的请求标识",
  PAIRING_EXPIRED: "配对码已经过期",
  PAIRING_UNAVAILABLE: "当前没有可用的伴生应用配对请求",
  PROVIDER_CLIENT_MISSING: "没有找到对应的 Agent 客户端",
  PROVIDER_INSTALL_REQUIRED: "请先安装对应的 Agent 客户端",
  QUESTION_EXPIRED: "这个问题已经过期，不能再提交",
  QUOTA_PERSIST_FAILED: "额度结果无法保存到本机",
  QUOTA_REFRESH_FAILED: "额度刷新失败",
  QUOTA_REFRESH_IN_PROGRESS: "额度正在刷新，请稍后再试",
  QUOTA_STATE_UNAVAILABLE: "额度状态暂不可用",
  REQUEST_MISMATCH: "请求与当前待处理事项不匹配",
  RETENTION_FAILED: "本地保留策略执行失败",
  RUNTIME_RESTART_FAILED: "Runtime 重启失败",
  RUNTIME_RESTART_TIMED_OUT: "Runtime 重启超时",
  RUNTIME_RESTART_UNAVAILABLE: "当前 Runtime 无法自动重启",
  RUNTIME_NOT_CONNECTED: "Runtime 尚未连接",
  RUNTIME_AUTH_FAILED: "Runtime 身份验证失败",
  RUNTIME_SESSION_MISSING: "Runtime 没有返回本机会话",
  SESSION_NOT_FOUND: "没有找到对应任务",
  TASK_STILL_ACTIVE: "任务仍在运行或等待处理，不能归档或删除历史",
  SETTINGS_READ_FAILED: "本机设置读取失败",
  SETUP_CHANGE_FAILED: "Agent 接入更新失败",
  SETUP_INSPECTION_FAILED: "Agent 接入状态检查失败",
  STALE_APPROVAL: "这项批准请求已经过期",
  STALE_ATTENTION: "这项待处理事项已经更新或过期",
  STORAGE_ERROR: "本地存储暂不可用",
  UNAUTHORIZED: "本机会话已失效，请重新连接",
  UNAUTHORIZED_MUTATION: "当前请求没有修改权限",
  UNAUTHORIZED_WEBSOCKET: "实时连接身份验证失败",
  UNKNOWN_ACTION: "未知操作",
  UNKNOWN_BRIDGE_ACTION: "未知额度桥操作",
  UNKNOWN_MANAGE_ACTION: "未知托管操作",
  UNKNOWN_PROVIDER: "未知 Agent 类型",
  UNKNOWN_SETUP_ACTION: "未知接入操作",
  UNSAFE_DATA_PATH: "本地数据目录未通过安全检查",
};

function runtimeMessageText(message, fallback = "") {
  return I18N.runtimeMessage(message, fallback);
}

function apiErrorText(error) {
  const code = String(error?.message || "UNKNOWN_ERROR");
  const localized = I18N.apiError(code);
  if (localized) return localized;
  const httpStatus = code.match(/^HTTP_(\d+)$/)?.[1];
  if (httpStatus) return currentLocale() === "en"
    ? `Local request failed (HTTP ${httpStatus})`
    : `本机请求失败（HTTP ${httpStatus}）`;
  if (code === "RESTART_TIMEOUT") return tr("Runtime 未能自动恢复");
  if (code.startsWith("HEALTH_")) return tr("Runtime 健康检查失败");
  return tr("请求失败，请重试");
}

const DISPLAY_FIELDS_ZH = agentDetail.displayFieldsZh;

const SOCKET_STALE_AFTER_MS = 25 * 1000;
const SNAPSHOT_FALLBACK_AFTER_MS = 15 * 1000;
const SETUP_FOCUS_REFRESH_AFTER_MS = 5 * 1000;
const USER_GUIDE_URL = "https://github.com/Frontier-Interfaces/ActRealm/blob/agent/v1-full/docs/USER_GUIDE_zh-CN.md";
const DISPLAY_PRESETS = agentDetail.presets;
const DISPLAY_FIELD_GROUPS = [
  { id: "headline", title: "主标题与状态", detail: "折叠任务卡的第一、二行" },
  { id: "subtitle", title: "副标题与进度", detail: "折叠任务卡的身份信息与计划" },
  { id: "overview", title: "用量概览", detail: "折叠任务卡中的用量胶囊" },
  { id: "details", title: "展开详情", detail: "点击任务卡后显示" },
  { id: "developer", title: "开发者信息", detail: "展开详情中的来源与内部标识" },
];

function updateClock() {
  const now = new Date();
  ui.menuClock.textContent = now.toLocaleTimeString([], { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = tr(String(text));
  return node;
}

function rawElement(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
}

function setClientText(node, text) {
  node.textContent = tr(String(text));
}

function initializeLanguage() {
  I18N.translateStatic(document);
  ui.languageSelect.value = I18N.preference();
  ui.languageSelect.addEventListener("change", () => I18N.setPreference(ui.languageSelect.value));
  window.addEventListener("actrealm:language-changed", () => {
    ui.languageSelect.value = I18N.preference();
    I18N.translateStatic(document);
    render(snapshot);
    renderSetup();
    renderSettings();
    setConnected(runtimeConnected);
    updateLiveTimes();
  });
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
  const brand = element("img", "onboarding-brand-icon");
  brand.src = "/assets/actrealm-icon.png";
  brand.alt = "";
  icon.append(brand);
  root.append(icon);
  root.append(element("h3", "", "尚未连接任何 Agent"));
  root.append(element("p", "", "连接 Claude 或 Codex 后，运行中的任务与待处理事项会显示在这里。数据仅留在本机。"));
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
  return snapshot.attention
    .filter((item) => visibleStates.has(item.state)
      && !agentState.isAcknowledgedCompletion(item)
      && notificationRule(item) !== "ignore")
    .sort((a, b) => agentState.attentionPriority(a) - agentState.attentionPriority(b)
      || a.createdAt - b.createdAt);
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

function providerCapabilityStatus(session, feature) {
  return snapshot.capabilities?.providerMatrix?.providers?.[session.provider]?.[feature]?.status || "unknown";
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
    error: { label: "配置无法解析", className: "error", detail: "为保护现有设置，ActRealm 已拒绝改写。请先恢复或修正配置。" },
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
    setClientText(ui.agentStatusLabel, "正在检测 Agent");
    setClientText(ui.setupTriggerLabel, "连接 Agent");
  } else if (isFirstRun()) {
    setClientText(ui.agentStatusLabel, "未连接 Agent");
    setClientText(ui.setupTriggerLabel, "连接 Agent");
  } else if (pending > 0) {
    setClientText(ui.agentStatusLabel, connected ? `${connected} 个已接入 · ${pending} 待处理` : `${pending} 项接入待处理`);
    setClientText(ui.setupTriggerLabel, "完成接入");
  } else {
    setClientText(ui.agentStatusLabel, `${connected} 个 Agent 已接入`);
    setClientText(ui.setupTriggerLabel, "Agent 接入");
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
    const configPath = provider.configPath
      ? rawElement("code", "", provider.configPath)
      : element("code", "", "尚未生成配置路径");
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
      trust.append(element("strong", "", "Codex 信任必须在官方界面确认"));
      const steps = element("ol", "trust-steps");
      const startStep = provider.cliInstalled
        ? "打开任意 Codex 终端会话"
        : `打开终端并运行内置 Codex：${provider.reviewCommand || "ChatGPT.app/Contents/Resources/codex"}`;
      for (const step of [startStep, "输入 /hooks", "核对命令路径后选择信任", "启动一个新会话并回到这里刷新"]) {
        steps.append(element("li", "", step));
      }
      trust.append(steps);
      if (provider.reviewCommand) trust.append(rawElement("code", "setup-review-command", provider.reviewCommand));
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
    showToast(`接入状态读取失败：${apiErrorText(error)}`);
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
    showToast(`接入操作失败：${apiErrorText(error)}`);
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

function formatBackupBytes(value) {
  let amount = Math.max(0, Number(value) || 0);
  const units = ["B", "KB", "MB", "GB"];
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || amount >= 10 ? 0 : 1;
  return `${amount.toFixed(digits)} ${units[unit]}`;
}

function renderSettings() {
  const rules = settingsState.notificationRules || {};
  const visibleRule = (value) => value === "ignore" ? "ignore" : "list";
  ui.notifyApproval.checked = visibleRule(rules.approval) === "list";
  ui.notifyQuestion.checked = visibleRule(rules.question) === "list";
  ui.notifyError.checked = visibleRule(rules.error) === "list";
  ui.notifyCompletion.checked = visibleRule(rules.completion) === "list";
  ui.soundEnabled.checked = Boolean(settingsState.soundEnabled);
  ui.muteClaude.checked = Boolean(settingsState.providerMuted?.claude);
  ui.muteCodex.checked = Boolean(settingsState.providerMuted?.codex);
  ui.codexEnhanced.checked = Boolean(settingsState.codexEnhancedActivity);
  const completionSettings = agentState.completionSettings(settingsState);
  ui.completionHideMode.value = completionSettings.mode;
  ui.completionAutoHideMinutes.value = String(completionSettings.minutes);
  ui.completionAutoHideRow.hidden = completionSettings.mode !== "afterDelay";
  const connector = snapshot.capabilities?.codexConnector;
  setClientText(ui.codexConnectorStatus, connector?.status === "connected"
    ? `已连接 · ${connector.managedThreads || 0} 个托管对话`
    : connector?.status === "disabled"
      ? "当前 Runtime 未启用"
      : connector?.error || "当前版本不可用");
  const retention = [30, 90, 180, 0].includes(Number(settingsState.retentionDays))
    ? Number(settingsState.retentionDays)
    : 180;
  ui.retentionDays.value = String(retention);
  ui.displayProfile.value = settingsState.displayProfile || "detailed";
  renderFieldSelector();
  setClientText(ui.claudeBridgeStatus, bridgeStatusCopy(claudeBridge.status));
  const removable = claudeBridge.status === "installed";
  const blocked = claudeBridge.status === "config_malformed";
  setClientText(ui.claudeBridgeAction, removable
    ? "关闭"
    : claudeBridge.status === "custom_conflict"
      ? "保留现有并开启"
      : claudeBridge.status === "helper_missing"
        ? "修复"
        : "开启");
  ui.claudeBridgeAction.dataset.action = removable
    ? "uninstall"
    : claudeBridge.status === "custom_conflict"
      ? "wrap"
      : "install";
  ui.claudeBridgeAction.disabled = settingsBusy || blocked;
  const backupCount = Number(backupSummary.count || 0);
  const backupCountLabel = currentLocale() === "en"
    ? `${backupCount.toLocaleString("en")} ${backupCount === 1 ? "backup" : "backups"}`
    : `${backupCount.toLocaleString("zh-Hans")} 个`;
  setClientText(
    ui.backupSummary,
    `${backupCountLabel} · ${formatBackupBytes(backupSummary.totalBytes)}`,
  );
  ui.clearBackups.disabled = settingsBusy || Number(backupSummary.count || 0) === 0;
  for (const button of ui.retentionOptions) {
    const active = Number(button.dataset.value) === retention;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  }
  for (const button of ui.quotaDisplayOptions) {
    const active = button.dataset.value === normalizedQuotaDisplayMode(settingsState.quotaDisplayMode);
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  }
}

function renderFieldSelector() {
  ui.taskCardFields.replaceChildren();
  const selected = new Set(settingsState.taskCardFields || []);
  const profile = settingsState.displayProfile || "detailed";
  for (const group of DISPLAY_FIELD_GROUPS) {
    const fields = displayCatalog.filter((field) => (field.placement || (field.level === "developer" ? "developer" : "details")) === group.id);
    if (!fields.length) continue;
    const section = element("section", "field-group");
    const heading = element("div", "field-group-heading");
    heading.append(element("strong", "", group.title), element("span", "", group.detail));
    section.append(heading);
    const options = element("div", "field-group-options");
    for (const field of fields) {
      const label = element("label", `field-option field-${field.level || "detailed"}`);
      const input = document.createElement("input");
      input.type = "checkbox";
      input.value = field.id;
      input.checked = selected.has(field.id);
      input.disabled = profile !== "custom";
      const copy = element("span", "field-option-copy");
      const localizedField = DISPLAY_FIELDS_ZH[field.id];
      const fieldLabel = localizedField?.[0] || field.id;
      const fieldDescription = localizedField?.[1] || "";
      copy.append(element("strong", "", fieldLabel));
      if (fieldDescription) copy.append(element("small", "", fieldDescription));
      label.append(input, copy);
      options.append(label);
    }
    section.append(options);
    ui.taskCardFields.append(section);
  }
}

function setSettingsFeedback(message, { error = false, retry = false } = {}) {
  setClientText(ui.settingsSaveMessage, message || "");
  ui.settingsSaveFeedback.hidden = !message;
  ui.settingsSaveFeedback.classList.toggle("error", error);
  ui.settingsSaveRetry.hidden = !retry;
}

async function loadSettings() {
  try {
    const response = await api("/api/v1/settings");
    settingsState = response.settings;
    failedSettingsDraft = undefined;
    setSettingsFeedback("");
    displayCatalog = response.displayCatalog || [];
    claudeBridge = response.claudeQuotaBridge;
    backupSummary = response.backups || { count: 0, totalBytes: 0 };
    renderSettings();
    renderAttention();
    renderSessions();
    renderQuota();
  } catch (error) {
    setSettingsFeedback(`设置读取失败：${apiErrorText(error)}`, { error: true, retry: true });
  }
}

function normalizedQuotaDisplayMode(value) {
  return ["standard", "twoLine", "compact"].includes(value) ? value : "standard";
}

function settingsFromForm() {
  return {
    notificationRules: {
      approval: ui.notifyApproval.checked ? "list" : "ignore",
      question: ui.notifyQuestion.checked ? "list" : "ignore",
      error: ui.notifyError.checked ? "list" : "ignore",
      completion: ui.notifyCompletion.checked ? "list" : "ignore",
    },
    soundEnabled: ui.soundEnabled.checked,
    providerMuted: { claude: ui.muteClaude.checked, codex: ui.muteCodex.checked },
    codexEnhancedActivity: ui.codexEnhanced.checked,
    retentionDays: Number(ui.retentionDays.value),
    displayProfile: ui.displayProfile.value,
    displayFieldsVersion: settingsState.displayFieldsVersion || 4,
    taskCardFields: [...ui.taskCardFields.querySelectorAll("input:checked")].map((input) => input.value),
    quotaDisplayMode: normalizedQuotaDisplayMode(settingsState.quotaDisplayMode),
    tokenUsageDisplayMode: settingsState.tokenUsageDisplayMode || "standard",
    tokenUsageComponentsVisible: settingsState.tokenUsageComponentsVisible !== false,
    tokenUsageHeatmapVisible: settingsState.tokenUsageHeatmapVisible !== false,
    tokenUsageCostVisible: settingsState.tokenUsageCostVisible !== false,
    tokenUsageObservedTimeVisible: settingsState.tokenUsageObservedTimeVisible !== false,
    tokenUsageExecutionTimeVisible: settingsState.tokenUsageExecutionTimeVisible !== false,
    tokenUsageUnitStyle: settingsState.tokenUsageUnitStyle || "automatic",
    ...window.TokenDecision?.settings(settingsState),
    completionTaskHideMode: ui.completionHideMode.value,
    completionAutoHideMinutes: Number(ui.completionAutoHideMinutes.value),
  };
}

async function saveSettings(settingsOverride) {
  if (settingsBusy) return;
  settingsBusy = true;
  const previousCodexMode = settingsState.codexEnhancedActivity;
  const draft = settingsOverride || settingsFromForm();
  setSettingsFeedback("");
  try {
    const response = await api("/api/v1/settings", {
      method: "PUT",
      body: JSON.stringify(draft),
    });
    settingsState = response.settings;
    failedSettingsDraft = undefined;
    displayCatalog = response.displayCatalog || displayCatalog;
    claudeBridge = response.claudeQuotaBridge;
    backupSummary = response.backups || backupSummary;
    renderSettings();
    renderAttention();
    renderSessions();
    renderQuota();
    if (previousCodexMode !== settingsState.codexEnhancedActivity) {
      setSettingsFeedback("Codex Hook 已更新，请在 Codex 中运行 /hooks 重新检查信任。");
      loadSetup();
    } else {
      setSettingsFeedback("");
    }
  } catch (error) {
    failedSettingsDraft = draft;
    renderSettings();
    setSettingsFeedback(`设置保存失败：${apiErrorText(error)}`, { error: true, retry: true });
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
    backupSummary = response.backups || backupSummary;
    await loadSnapshot();
    showToast(action === "uninstall" ? "Claude 额度桥已关闭，原状态栏已恢复" : "Claude 额度桥已开启，完成一次对话后会显示额度");
  } catch (error) {
    showToast(`额度桥操作失败：${apiErrorText(error)}`);
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
    showToast(`导出失败：${apiErrorText(error)}`);
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
    showToast(`统计导出失败：${apiErrorText(error)}`);
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
    showToast(`清除失败：${apiErrorText(error)}`);
  }
}

function openBackupClearConfirmation() {
  ui.backupWipeConfirmation.hidden = false;
  ui.backupWipeConfirmationInput.value = "";
  ui.backupWipeConfirmationInput.focus();
}

function cancelBackupClearConfirmation() {
  ui.backupWipeConfirmation.hidden = true;
  ui.backupWipeConfirmationInput.value = "";
}

async function clearConfigurationBackups() {
  const confirmation = ui.backupWipeConfirmationInput.value.trim();
  if (confirmation !== "DELETE BACKUPS") {
    showToast("请输入 DELETE BACKUPS；没有删除任何备份");
    return;
  }
  try {
    const response = await api("/api/v1/backups/clear", {
      method: "POST",
      body: JSON.stringify({ confirmation }),
    });
    backupSummary = response.backups || { count: 0, totalBytes: 0 };
    cancelBackupClearConfirmation();
    renderSettings();
    showToast("ActRealm 配置备份已清除");
  } catch (error) {
    showToast(`${tr("备份清除失败：")}${apiErrorText(error)}`);
  }
}

function stateLabel(state) {
  return tr({
    open: "等待处理",
    committing: "3 秒内可撤回",
    decision_sent: "决定已发送",
    confirmed: "已确认继续",
    resolved: "已解决",
    passed_through: "已交回终端",
    expired: "已过期，交回终端",
    snoozed: "稍后提醒",
    dismissed: "已忽略",
  }[state] || state);
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
  if (item.kind === "approval") {
    const command = item.commandPreview || tr("一项工具操作");
    return currentLocale() === "en"
      ? `Request to run ${command}; waiting for approval`
      : `请求运行 ${command}，等待批准`;
  }
  return runtimeMessageText(
    item.titleMessage,
    item.title || {
      native_approval: currentLocale() === "en"
        ? `${providerName(item.provider)} is waiting for approval in the original interface`
        : `${providerName(item.provider)} 等待在原界面批准`,
      error: tr("任务出错停下来了"),
      completion: tr("这一轮已经完成"),
    }[item.kind] || tr("Agent 有一项待处理事项"),
  );
}

function attentionContext(item) {
  if (item.kind === "native_approval") {
    return {
      kicker: currentLocale() === "en"
        ? `${providerName(item.provider)} request in original interface · ActRealm only syncs status`
        : `${providerName(item.provider)} 原界面请求 · ActRealm 仅同步状态`,
      state: tr("等待原界面处理"),
      notification: tr("原界面请求批准"),
    };
  }
  if (item.kind === "approval") {
    return {
      kicker: tr("可在 ActRealm 审批 · 任务等待决定"),
      state: stateLabel(item.state),
      notification: tr("可在 ActRealm 审批"),
    };
  }
  if (item.kind === "question") {
    return {
      kicker: tr("可在 ActRealm 回答 · 任务等待输入"),
      state: stateLabel(item.state),
      notification: tr("等待回答"),
    };
  }
  return {
    kicker: tr(item.kind === "completion" ? "任务已完成" : "需要处理 · 任务已暂停"),
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
    showToast(`回答失败：${apiErrorText(error)}`);
  }
}

function renderInteractiveForm(item, card) {
  const interaction = item.interaction;
  if (!interaction || item.state !== "open" || !item.requestId) return false;
  if (interaction.message) card.append(rawElement("div", "fact-block question-message", interaction.message));
  const form = element("form", "question-form");
  const bindings = [];
  const allControls = [];
  for (const question of interaction.questions || []) {
    const fieldset = element("fieldset", "question-field");
    const legend = question.label
      ? rawElement("legend", "", question.label)
      : element("legend", "", "问题");
    fieldset.append(legend);
    if (question.prompt) fieldset.append(rawElement("p", "question-prompt", question.prompt));
    const binding = { question, values: [], other: undefined, input: undefined, error: undefined };
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
        copy.append(rawElement("strong", "", option.label));
        if (option.description) copy.append(rawElement("small", "", option.description));
        label.append(input, copy);
        choices.append(label);
      }
      if (question.allowsOther) {
        const other = document.createElement("input");
        other.type = "text";
        other.className = "question-other";
        other.placeholder = tr("其他答案（可直接输入）");
        other.maxLength = 2000;
        allControls.push(other);
        binding.other = other;
        choices.append(other);
      }
      fieldset.append(choices);
    } else if (question.inputType === "boolean") {
      const input = document.createElement("select");
      input.append(new Option(tr("请选择"), ""), new Option(tr("是"), "true"), new Option(tr("否"), "false"));
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
    binding.error = element("p", "question-error");
    binding.error.hidden = true;
    fieldset.append(binding.error);
    for (const control of [...binding.values, binding.other, binding.input].filter(Boolean)) {
      const clearError = () => {
        binding.error.hidden = true;
        binding.error.textContent = "";
        fieldset.classList.remove("invalid");
      };
      control.addEventListener("input", clearError);
      control.addEventListener("change", clearError);
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
    const showFieldError = (binding, message) => {
      setClientText(binding.error, message);
      binding.error.hidden = false;
      binding.error.parentElement.classList.add("invalid");
      const target = binding.input || binding.values[0] || binding.other;
      target?.focus();
    };
    for (const binding of bindings) {
      binding.error.hidden = true;
      binding.error.textContent = "";
      binding.error.parentElement.classList.remove("invalid");
    }
    for (const binding of bindings) {
      const { question } = binding;
      if (question.inputType === "choice") {
        const values = binding.values.filter((input) => input.checked).map((input) => input.value);
        if (binding.other?.value.trim()) values.push(binding.other.value.trim());
        if (!values.length && question.required) {
          showFieldError(binding, "请选择一个答案。");
          return;
        }
        answers[question.id] = ["claude_question", "codex_user_input"].includes(interaction.kind) ? values : values[0];
      } else if (question.inputType === "boolean") {
        if (!binding.input.value && question.required) {
          showFieldError(binding, "请选择“是”或“否”。");
          return;
        }
        if (binding.input.value) answers[question.id] = interaction.kind === "codex_user_input" ? [binding.input.value] : binding.input.value === "true";
      } else {
        const value = binding.input.value.trim();
        if (!value && question.required) {
          showFieldError(binding, "请输入回答。");
          return;
        }
        if (value) {
          const normalized = question.inputType === "number" ? Number(value) : value;
          if (question.inputType === "number" && !Number.isFinite(normalized)) {
            showFieldError(binding, "请输入有效数字。");
            return;
          }
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
    currentAttentionID,
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
    setClientText(ui.attentionSummary, `最久等待 ${minutes} 分钟`);
  } else {
    setClientText(ui.attentionSummary, "暂无需要处理的事项");
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
  const previous = items.find((candidate) => candidate.id === currentAttentionID);
  const item = previous
    && agentState.attentionPriority(previous) <= agentState.attentionPriority(items[0])
    ? previous
    : items[0];
  currentAttentionID = item.id;
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
  card.append(kicker, rawElement("h3", "", attentionTitle(item)));

  const agentLine = element("div", "agent-line");
  agentLine.append(providerIcon(item.provider));
  agentLine.append(element("strong", "", providerName(item.provider)));
  if (item.project) agentLine.append(rawElement("span", "", `· ${item.project}`));
  const session = snapshot.sessions.find((candidate) => candidate.id === item.sessionId);
  if (session?.title) agentLine.append(rawElement("span", "", session.title));
  card.append(agentLine);

  const taskJump = element("button", "task-jump", "在 Agent 任务中查看 →");
  taskJump.type = "button";
  taskJump.addEventListener("click", () => selectSession(item.sessionId));
  card.append(taskJump);

  const interactive = renderInteractiveForm(item, card);
  if (!interactive) {
    const fact = runtimeMessageText(item.detailMessage, item.detail) || item.commandPreview;
    if (fact) card.append(element("div", "fact-block", fact));
    const risk = element("div", "risk-row");
    risk.append(element("span", "risk-chip", `风险标记：${item.risk || "未知"}`));
    for (const [index, note] of (item.riskNotes || []).entries()) {
      risk.append(element(
        "span",
        "risk-chip",
        runtimeMessageText(item.riskMessages?.[index], note),
      ));
    }
    if (item.expiresAt) risk.append(element("span", "risk-chip", `截止 ${new Date(item.expiresAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`));
    if (item.autoHideAt) risk.append(element("span", "risk-chip", `自动隐藏 ${new Date(item.autoHideAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`));
    card.append(risk);
  }

  const actions = element("div", "actions");
  if (interactive) {
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
    const acknowledge = item.kind === "completion"
      ? (item.autoHideAt ? "知道了" : "确认完成")
      : "标记已解决";
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
      if (candidate.id === currentAttentionID) return;
      const row = element("button", "queue-item");
      row.type = "button";
      row.append(
        element("span", `queue-kind ${candidate.kind || "approval"}`, {
          approval: "等待批准", native_approval: "原界面批准", question: "提问", completion: "完成", error: "错误",
        }[candidate.kind] || "待处理"),
        rawElement("strong", "", attentionTitle(candidate)),
        markLiveElapsed(element("span", ""), candidate.createdAt),
      );
      row.addEventListener("click", () => { currentAttentionID = candidate.id; renderAttention(); });
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
    if (first.kind === "native_approval") return { label: `${tr("原界面请求")}${suffix}`, className: "waiting" };
    if (first.kind === "approval") return { label: `${tr("面板可审批")}${suffix}`, className: "waiting" };
    if (first.kind === "question") return { label: `${tr("等待回答")}${suffix}`, className: "waiting" };
    if (first.kind === "completion") return { label: `${tr("待确认")}${suffix}`, className: "waiting" };
    return { label: `${tr("待处理")}${suffix}`, className: "waiting" };
  }
  if (session.execState === "failed") return { label: tr("出错"), className: "failed" };
  const completion = pendingAttentionForSession(session).find((item) => item.kind === "completion");
  if (!isSessionActive(session) && completion) {
    return completion.reminderAcknowledgedAt ? { label: tr("已完成"), className: "idle" }
      : { label: tr("待确认"), className: "waiting" };
  }
  if (["idle", "response_finished"].includes(session.execState)) return { label: tr("空闲"), className: "idle" };
  return { label: tr("在跑"), className: "" };
}

function recoveryDisplay(session) {
  const result = {
    controllable: { label: "已重新连接，可控制", className: "controllable" },
    observing: { label: "仍在运行，仅可观察", className: "observing" },
    waiting_for_event: { label: "历史已恢复，等待新事件", className: "waiting" },
    lost_control: { label: "已失去控制", className: "lost" },
    ended: { label: "已结束", className: "ended" },
  }[session.recoveryState] || { label: "等待确认状态", className: "waiting" };
  return { ...result, label: tr(result.label) };
}

function elapsedText(since, until = Date.now()) {
  const seconds = Math.max(0, Math.floor((Number(until) - Number(since || until)) / 1000));
  if (seconds < 60) return currentLocale() === "en" ? I18N.plural(seconds, "second") : `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return currentLocale() === "en"
    ? `${I18N.plural(minutes, "minute")} ${I18N.plural(seconds % 60, "second")}`
    : `${minutes} 分 ${seconds % 60} 秒`;
  return currentLocale() === "en"
    ? `${I18N.plural(Math.floor(minutes / 60), "hour")} ${I18N.plural(minutes % 60, "minute")}`
    : `${Math.floor(minutes / 60)} 小时 ${minutes % 60} 分`;
}

function markLiveElapsed(node, since, prefix = "", suffix = "") {
  node.dataset.liveElapsedSince = String(Number(since || Date.now()));
  node.dataset.liveElapsedPrefix = prefix;
  node.dataset.liveElapsedSuffix = suffix;
  node.textContent = `${prefix === "已等 " && currentLocale() === "en" ? "Waiting " : tr(prefix)}${elapsedText(node.dataset.liveElapsedSince)}${tr(suffix)}`;
  return node;
}

function updateMarkedElapsed(root) {
  for (const node of root.querySelectorAll("[data-live-elapsed-since]")) {
    const prefix = node.dataset.liveElapsedPrefix || "";
    node.textContent = `${prefix === "已等 " && currentLocale() === "en" ? "Waiting " : tr(prefix)}${elapsedText(node.dataset.liveElapsedSince)}${tr(node.dataset.liveElapsedSuffix || "")}`;
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
    ? currentLocale() === "en"
      ? ` · Current phase ${elapsedText(session.activitySince)}`
      : ` · 当前阶段 ${elapsedText(session.activitySince)}`
    : "";
  return currentLocale() === "en" ? `Turn ${total}${stage}` : `本轮 ${total}${stage}`;
}

function compactCount(value) {
  const count = Number(value || 0);
  if (count >= 1_000_000) return `${Math.round(count / 100_000) / 10}m`;
  if (count >= 1_000) return `${Math.round(count / 100) / 10}k`;
  return String(count);
}

function contextUsage(session) {
  const used = session.contextUsedTokens == null ? undefined : Number(session.contextUsedTokens);
  const windowSize = Number(session.contextWindowTokens || 0);
  const providerPercent = Number(session.contextUsedPercent);
  const percent = Number.isFinite(providerPercent) && session.contextUsedPercent != null
    ? Math.max(0, Math.min(100, providerPercent))
    : used != null && used > 0 && windowSize > 0
      ? Math.max(0, Math.min(100, Math.round(used / windowSize * 100)))
      : undefined;
  return { used, windowSize, percent };
}

function contextUsageText(session) {
  const context = contextUsage(session);
  if (!context.windowSize && context.percent == null) return "—";
  if (context.used != null && context.windowSize > 0) {
    return `${compactCount(context.used)} / ${compactCount(context.windowSize)} · ${context.percent}%`;
  }
  return context.percent == null ? `— / ${compactCount(context.windowSize)}` : `${context.percent}%`;
}

function estimatedCostText(session) {
  if (session.estimatedCostUsdMicros == null) return undefined;
  const dollars = Number(session.estimatedCostUsdMicros) / 1_000_000;
  if (!Number.isFinite(dollars) || dollars < 0) return undefined;
  if (dollars > 0 && dollars < 0.01) return `$${dollars.toFixed(4)}`;
  return `$${dollars.toFixed(2)}`;
}

function appendUsageStrip(container, session) {
  const total = cardFieldVisible("sessionTokens") && session.tokenTotal != null
    ? compactCount(session.tokenTotal)
    : undefined;
  const context = contextUsage(session);
  const contextPercent = cardFieldVisible("context") ? context.percent : undefined;
  const cost = cardFieldVisible("cost") ? estimatedCostText(session) : undefined;
  if (total == null && contextPercent == null && cost == null) return;
  const strip = element("div", "session-usage-strip");
  if (total != null) strip.append(element("span", "usage-chip", `累计 ${total} Token`));
  if (contextPercent != null) {
    const chip = element("span", "usage-chip context", `上下文 ${contextPercent}%`);
    chip.title = contextUsageText(session);
    strip.append(chip);
  }
  if (cost != null) {
    const chip = element("span", "usage-chip", `估算 API 价格 ${cost}`);
    chip.title = tr("按公开 API 单价或 Provider 官方会话费用估算；不是订阅账单");
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
  row.append(element("span", "", label), rawElement("strong", "", value));
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
  ui.sessionDetailTitle.textContent = session.providerTitle || session.title
    || (session.project ? `${providerName(session.provider)} · ${session.project}` : tr("任务详情"));
  ui.sessionDetailBody.replaceChildren();
  const fields = new Set(settingsState.taskCardFields || []);
  const status = sessionStatus(session);
  const recovery = recoveryDisplay(session);
  const rows = [
    detailPair("Provider", providerName(session.provider)),
    detailPair("状态", status.label),
    fields.has("recovery") ? detailPair("恢复状态", recovery.label) : undefined,
    fields.has("control") ? detailPair("控制能力", tr(session.controlCapability === "managed" ? "Codex app-server 托管，可回答提问" : "外部 Hook，仅观察/授权")) : undefined,
    fields.has("project") ? detailPair("项目", session.project || tr("项目未知")) : undefined,
    fields.has("task") ? detailPair("任务", session.title) : undefined,
    fields.has("model") ? detailPair("模型", session.model) : undefined,
    fields.has("activity") ? detailPair("实时活动", activityDisplay(session).text) : undefined,
    fields.has("plan") && Number.isInteger(session.planTotal) && session.planTotal > 0
      ? detailPair("计划", `${session.planDone || 0}/${session.planTotal}`)
      : undefined,
    fields.has("sessionTokens") && session.tokenTotal !== undefined && session.tokenTotal !== null
      ? detailPair("会话累计 Token", compactCount(session.tokenTotal))
      : undefined,
    fields.has("context") && Number(session.contextWindowTokens) > 0
      ? detailPair("本轮上下文", contextUsageText(session))
      : undefined,
    fields.has("inputOutputTokens") ? detailPair("输入 / 输出", session.inputTokens == null && session.outputTokens == null
      ? undefined
      : `${compactCount(session.inputTokens || 0)} / ${compactCount(session.outputTokens || 0)}`) : undefined,
    fields.has("cacheTokens") ? detailPair("缓存读取 / 写入", session.cacheReadTokens == null && session.cacheCreationTokens == null
      ? undefined
      : `${compactCount(session.cacheReadTokens || 0)} / ${compactCount(session.cacheCreationTokens || 0)}`) : undefined,
    fields.has("reasoningTokens") ? detailPair("推理 Token", session.reasoningTokens == null ? undefined : compactCount(session.reasoningTokens)) : undefined,
    fields.has("turnTokens") ? detailPair("本轮 Token", session.lastTurnTokens == null ? undefined : compactCount(session.lastTurnTokens)) : undefined,
    fields.has("sessionTokens") ? detailPair("Token 数据", agentDetail.usageFact(session, currentLocale())) : undefined,
    fields.has("cost") ? detailPair("估算 API 价格", estimatedCostText(session)) : undefined,
    fields.has("tool") ? detailPair("当前动作", agentDetail.currentAction(session, currentLocale())) : undefined,
    fields.has("currentTarget") ? detailPair("当前文件 / 目标", agentDetail.currentTarget(
      session,
      providerCapabilityStatus(session, "currentTarget"),
      currentLocale(),
    )) : undefined,
    fields.has("permissionMode") ? detailPair("权限模式", session.permissionMode) : undefined,
    fields.has("subagents") ? detailPair("运行中的子 Agent", agentDetail.subagentText(session, providerCapabilityStatus(session, "subagents"), currentLocale())) : undefined,
    fields.has("environment") ? detailPair("运行环境", session.environment) : undefined,
    fields.has("jump") ? detailPair(
      "跳转能力",
      runtimeMessageText(session.jumpMessage, session.jumpLabel),
    ) : undefined,
    fields.has("titleSource") ? detailPair("标题来源", agentDetail.titleSource(session.providerTitleSource, currentLocale())) : undefined,
    fields.has("sessionId") ? detailPair("ActRealm Session ID", session.id, "developer-value") : undefined,
    fields.has("providerSessionId") ? detailPair("Provider Session ID", session.providerSessionId, "developer-value") : undefined,
    fields.has("providerTurnId") ? detailPair("Provider Turn ID", session.providerTurnId, "developer-value") : undefined,
    fields.has("lastEventAt") ? detailPair("最后事件", new Date(session.lastEventAt).toLocaleString()) : undefined,
  ].filter(Boolean);
  const grid = element("div", "session-detail-grid");
  for (const row of rows) grid.append(row);
  ui.sessionDetailBody.append(grid);
  ui.sessionDetailJump.textContent = runtimeMessageText(
    session.jumpMessage,
    session.jumpLabel ? tr(session.jumpLabel) : tr("当前环境不支持跳转"),
  );
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
      if (agentState.isTaskDeleted(session, hiddenAt, snapshot.attention)) return false;
      const active = !["idle", "response_finished", "failed"].includes(session.execState);
      return active || Number(session.lastEventAt || 0) >= cutoff || attentionSessions.has(session.id);
    })
    .sort((a, b) => agentState.compareSessions(a, b, snapshot.attention, selectedSessionId));
}

function activityDisplay(session) {
  const waiting = blockingAttentionForSession(session)[0];
  if (waiting) {
    const text = waiting.kind === "native_approval"
      ? currentLocale() === "en"
        ? `${providerName(waiting.provider)} is requesting approval; handle it in the original interface`
        : `${providerName(waiting.provider)} 正在请求批准，请回原界面处理`
      : waiting.kind === "approval"
        ? tr("等待在 ActRealm 审批")
        : waiting.kind === "question"
          ? tr("等待在 ActRealm 回答")
          : tr("等待处理");
    return {
      className: "waiting",
      marker: "!",
      text: currentLocale() === "en"
        ? `${text} · ${turnTiming(session)} · Waiting ${elapsedText(waiting.createdAt)}`
        : `${text} · ${turnTiming(session)} · 已等 ${elapsedText(waiting.createdAt)}`,
    };
  }
  const completion = pendingAttentionForSession(session).find((item) => item.kind === "completion");
  if (!isSessionActive(session) && completion) {
    if (completion.reminderAcknowledgedAt) {
      return agentState.acknowledgedCompletionActivity(currentLocale(), turnTiming(session));
    }
    return {
      className: "waiting",
      marker: "✓",
      text: currentLocale() === "en"
        ? `Turn complete, waiting for confirmation · ${turnTiming(session)} · Waiting ${elapsedText(completion.createdAt)}`
        : `本轮已完成，等待确认 · ${turnTiming(session)} · 已等 ${elapsedText(completion.createdAt)}`,
    };
  }
  const timing = turnTiming(session);
  const runtimeActivity = runtimeMessageText(session.activityMessage, session.activity || "");
  if (session.execState === "thinking") {
    return { className: "thinking", marker: "•••", text: `${runtimeActivity || tr("正在思考")} · ${timing}` };
  }
  if (session.execState === "tool_running") {
    return { className: "tool", marker: "▌", text: `${agentDetail.currentAction(session, currentLocale()) || runtimeActivity || tr("正在运行工具")} · ${timing}` };
  }
  if (session.execState === "compacting") {
    return { className: "compacting", marker: "◌", text: `${runtimeActivity || tr("正在压缩记忆")} · ${timing}` };
  }
  if (session.execState === "failed") {
    return { className: "failed", marker: "×", text: `${runtimeActivity || tr("运行失败")} · ${timing}` };
  }
  if (session.execState === "response_finished") {
    return { className: "idle", marker: "✓", text: `${runtimeActivity || tr("本轮已完成")} · ${timing}` };
  }
  return { className: "idle", marker: "·", text: currentLocale() === "en"
    ? `${runtimeActivity || tr("空闲")} · ${elapsedText(session.lastEventAt)} ago`
    : `${runtimeActivity || "空闲"} · ${elapsedText(session.lastEventAt)}前` };
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
    showToast("当前环境不支持跳转；ActRealm 不会假装已定位到原对话");
    return;
  }
  try {
    const result = await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/jump`, {
      method: "POST",
      body: "{}",
    });
    if (result.success) {
      showToast(runtimeMessageText(
        result.labelMessage || session.jumpMessage,
        result.label || session.jumpLabel || "已打开 Agent",
      ));
    }
  } catch (error) {
    showToast(`跳转失败：${apiErrorText(error)}`);
  }
}

async function manageSession(session) {
  if (!session?.canManage) return;
  try {
    await api(`/api/v1/sessions/${encodeURIComponent(session.id)}/manage`, {
      method: "POST",
      body: JSON.stringify({ action: "attach" }),
    });
    showToast("已连接 ActRealm app-server；Codex 原生窗口仍保留当前 Turn 的控制权");
    await loadSnapshot();
  } catch (error) {
    showToast(`托管连接失败：${apiErrorText(error)}`);
  }
}

function activateSession(session) {
  selectSession(session.id);
  void jumpSession(session);
}

function clearSessionFromList(session) {
  const next = { ...hiddenSessions,
    [session.id]: agentState.deletionWatermark(session, Date.now(), snapshot.attention) };
  try { localStorage.setItem("actrealm.hiddenSessions", JSON.stringify(next)); }
  catch { showToast(tr("无法保存任务显示设置")); return; }
  hiddenSessions = next;
  if (selectedSessionId === session.id) selectedSessionId = undefined;
  renderSessions();
  showToast(tr("已删除任务 · 收到该会话的新事件后自动显示"));
}

function sessionRenderSignature(session) {
  return JSON.stringify({
    session,
    attention: pendingAttentionForSession(session),
    selected: session.id === selectedSessionId,
    fields: settingsState.taskCardFields || [],
  });
}

function buildSessionRow(session) {
    const fields = new Set(settingsState.taskCardFields || []);
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
    const clientTitle = session.providerTitle || session.title
      || (session.project ? `${providerName(session.provider)} · ${session.project}` : "等待下一条任务");
    title.append(fields.has("task")
      ? rawElement("strong", "", clientTitle)
      : element("strong", "", providerName(session.provider)));
    title.append(element("span", `state-pill ${status.className}`.trim(), status.label));
    if (fields.has("activity")) {
      const activity = activityDisplay(session);
      const activityNode = element("span", `task-right ${activity.className}`, activity.text);
      sessionActivityRefs.set(session.id, { root: activityNode });
      title.append(activityNode);
    }
    const taskContent = fields.has("task") && session.providerTitle && session.title && session.providerTitle !== session.title
      ? rawElement("div", "session-question", `${currentLocale() === "en" ? "Task summary" : "任务摘要"} · ${session.title}`)
      : undefined;
    copy.append(title);
    if (taskContent) copy.append(taskContent);
    const meta = [providerName(session.provider)];
    if (fields.has("project")) meta.push(session.project || tr("项目未知"));
    if (fields.has("model")) meta.push(session.model || tr("模型未知"));
    if (fields.has("project") || fields.has("model")) {
      copy.append(rawElement("div", "session-meta", meta.join(" · ")));
    }
    appendUsageStrip(copy, session);
    if (fields.has("plan") && Number.isInteger(session.planDone) && Number.isInteger(session.planTotal) && session.planTotal > 0) {
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
      if (fields.has("subagents")) {
        progress.append(element("span", "", agentDetail.subagentText(session, providerCapabilityStatus(session, "subagents"), currentLocale())));
      }
      copy.append(progress);
    }
    const clear = element("button", "session-clear", tr("删除任务"));
    clear.type = "button";
    clear.title = tr("仅删除任务卡，不停止 Agent，也不删除原会话和 Token 记录");
    clear.addEventListener("click", (event) => {
      event.stopPropagation();
      clearSessionFromList(session);
    });
    copy.append(clear);

    if (session.id === selectedSessionId) {
      const details = element("div", "task-expanded");
      const plan = Number(session.planTotal || 0) > 0 ? tr(`${session.planDone || 0}/${session.planTotal}（进行中）`) : tr("未提供");
      const pairs = [
        fields.has("environment") ? ["工作区", session.environment || session.project || `${providerName(session.provider)} 客户端`] : undefined,
        fields.has("context") ? ["本轮上下文", contextUsageText(session)] : undefined,
        fields.has("plan") ? ["计划", plan] : undefined,
        fields.has("sessionTokens") ? ["会话累计 Token", session.tokenTotal == null ? "—" : compactCount(session.tokenTotal)] : undefined,
        fields.has("turnTokens") ? ["本轮 Token", session.lastTurnTokens == null ? "—" : compactCount(session.lastTurnTokens)] : undefined,
        fields.has("sessionTokens") ? ["Token 数据", agentDetail.usageFact(session, currentLocale())] : undefined,
        fields.has("inputOutputTokens") ? ["输入 / 输出 Token", session.inputTokens == null && session.outputTokens == null
          ? "—"
          : `${compactCount(session.inputTokens || 0)} / ${compactCount(session.outputTokens || 0)}`] : undefined,
        fields.has("cacheTokens") ? ["缓存读取 / 写入 Token", session.cacheReadTokens == null && session.cacheCreationTokens == null
          ? "—"
          : `${compactCount(session.cacheReadTokens || 0)} / ${compactCount(session.cacheCreationTokens || 0)}`] : undefined,
        fields.has("reasoningTokens") ? ["推理 Token", session.reasoningTokens == null ? "—" : compactCount(session.reasoningTokens)] : undefined,
        fields.has("cost") ? ["估算 API 价格", estimatedCostText(session) || "—"] : undefined,
        fields.has("tool") ? ["当前动作", agentDetail.currentAction(session, currentLocale())] : undefined,
        fields.has("currentTarget") ? ["当前文件 / 目标", agentDetail.currentTarget(
          session,
          providerCapabilityStatus(session, "currentTarget"),
          currentLocale(),
        )] : undefined,
        fields.has("permissionMode") ? ["权限模式", session.permissionMode || "—"] : undefined,
        fields.has("subagents") ? ["运行中的子 Agent", agentDetail.subagentText(session, providerCapabilityStatus(session, "subagents"), currentLocale())] : undefined,
        fields.has("recovery") ? ["恢复状态", recoveryDisplay(session).label] : undefined,
        fields.has("control") ? ["托管能力", tr(session.controlCapability === "managed" ? "Codex app-server 托管，可回答提问" : "外部 Hook，仅观察/授权")] : undefined,
        fields.has("jump") ? ["打开应用", runtimeMessageText(
          session.jumpMessage,
          session.jumpLabel ? tr(session.jumpLabel) : tr("当前环境不支持"),
        )] : undefined,
        fields.has("titleSource") ? ["标题来源", agentDetail.titleSource(session.providerTitleSource, currentLocale())] : undefined,
        fields.has("sessionId") ? ["ActRealm Session ID", session.id] : undefined,
        fields.has("providerSessionId") ? ["Provider Session ID", session.providerSessionId] : undefined,
        fields.has("providerTurnId") ? ["Provider Turn ID", session.providerTurnId || "—"] : undefined,
        fields.has("lastEventAt") ? ["最后事件", new Date(session.lastEventAt).toLocaleString()] : undefined,
      ].filter(Boolean);
      for (const [label, value] of pairs) {
        const pair = element("div", "task-detail-pair");
        pair.append(element("span", "", label), rawElement("strong", "", value));
        details.append(pair);
      }
      agentDetail.renderSections(details, session, fields, {
        locale: currentLocale(),
        api,
        planCapability: providerCapabilityStatus(session, "plan"),
        workflowCapability: providerCapabilityStatus(session, "toolLifecycle"),
      });
      const waiting = openItems().find((item) => item.sessionId === session.id);
      if (waiting) {
        const locate = element("button", "task-locate", "查看待处理事项");
        locate.type = "button";
        locate.addEventListener("click", (event) => {
          event.stopPropagation();
          currentAttentionID = waiting.id;
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
  setClientText(ui.sessionCount, `${sessions.length} 个任务 · ${waitingCount} 等待 · ${runningCount} 运行中 · ${finishedCount} 已完成`);
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
  if (currentLocale() === "en") {
    if (value > 0 && value % 43200 === 0) return I18N.plural(value / 43200, "month");
    if (value > 0 && value % 10080 === 0) return I18N.plural(value / 10080, "week");
    if (value > 0 && value % 1440 === 0) return I18N.plural(value / 1440, "day");
    if (value > 0 && value % 60 === 0) return I18N.plural(value / 60, "hour");
    if (value > 0) return I18N.plural(value, "minute");
    if (fallback === "5h") return "5 hours";
    if (fallback === "7d") return "7 days";
    return fallback && fallback !== "unknown" ? fallback.replaceAll("_", " ") : "Quota";
  }
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
  const name = runtimeMessageText(
    quota.windowMessage,
    quota.limitName || quotaDurationLabel(quota.windowMinutes, quota.window),
  );
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
  return JSON.stringify({
    displayMode: normalizedQuotaDisplayMode(settingsState.quotaDisplayMode),
    slots: quotaSlots(),
  });
}

function updateQuotaTimes() {
  for (const node of ui.quotaList.querySelectorAll("[data-quota-captured-at]")) {
    const minutes = Math.max(0, Math.floor((Date.now() - Number(node.dataset.quotaCapturedAt)) / 60000));
    setClientText(node, minutes > 0 ? `${minutes} 分钟前更新` : "刚刚更新");
  }
  for (const node of ui.quotaList.querySelectorAll("[data-quota-resets-at]")) {
    const reset = new Date(Number(node.dataset.quotaResetsAt));
    node.textContent = reset.getTime() <= Date.now()
      ? tr("已到重置时间，等待同步")
      : currentLocale() === "en"
        ? `Resets ${reset.toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit" })}`
        : `${reset.toLocaleString([], { weekday: "short", hour: "2-digit", minute: "2-digit" })} 重置`;
  }
}

function renderQuota() {
  lastQuotaRenderSignature = quotaRenderSignature();
  ui.quotaList.replaceChildren();
  const displayMode = normalizedQuotaDisplayMode(settingsState.quotaDisplayMode);
  const compact = displayMode === "twoLine";
  const singleLine = displayMode === "compact";
  ui.quotaList.classList.toggle("compact", compact);
  ui.quotaList.classList.toggle("single-line", singleLine);
  if (isFirstRun()) {
    setClientText(ui.quotaSyncTime, "最近同步 · 等待 Agent 接入");
    ui.quotaList.append(onboardingQuotaState());
    return;
  }
  const slots = quotaSlots();
  const latestCapture = Math.max(0, ...slots.map((quota) => Number(quota.capturedAt || 0)));
  ui.quotaSyncTime.textContent = latestCapture
    ? currentLocale() === "en"
      ? `Last sync · ${new Date(latestCapture).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`
      : `最近同步 · ${new Date(latestCapture).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`
    : tr("最近同步 · 等待额度来源");
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
      const modeClass = compact ? " compact" : singleLine ? " single-line" : "";
      const unavailable = element("article", `quota-unavailable${modeClass}`);
      const unavailableTitle = element("div", "row-title");
      unavailableTitle.append(providerIcon(quota.provider), element("strong", "", label));
      if (compact) {
        unavailableTitle.append(element("span", "quota-status-chip neutral", "暂不可用"));
        unavailable.append(unavailableTitle);
        const detail = element("div", "quota-compact-detail unavailable");
        detail.append(element(
          "p",
          "",
          runtimeMessageText(quota.reasonMessage, quota.reason)
            || "额度来源没有返回可验证数据",
        ));
        if (quota.provider === "claude") {
          const help = element("button", "quota-help", "检查设置");
          help.type = "button";
          help.addEventListener("click", openSettings);
          detail.append(help);
        }
        unavailable.append(detail);
        ui.quotaList.append(unavailable);
        continue;
      }
      if (singleLine) {
        unavailableTitle.append(
          element("span", "quota-compact-status", "暂不可用"),
          element("span", "quota-state-dot neutral"),
        );
        unavailable.title = runtimeMessageText(quota.reasonMessage, quota.reason)
          || "额度来源没有返回可验证数据";
        if (quota.provider === "claude") {
          unavailable.tabIndex = 0;
          unavailable.setAttribute("role", "button");
          unavailable.addEventListener("click", openSettings);
          unavailable.addEventListener("keydown", (event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              openSettings();
            }
          });
        }
        unavailable.append(unavailableTitle);
        ui.quotaList.append(unavailable);
        continue;
      }
      unavailable.append(unavailableTitle);
      unavailable.append(element(
        "p",
        "",
        runtimeMessageText(quota.reasonMessage, quota.reason)
          || "额度来源没有返回可验证数据",
      ));
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
    const modeClass = compact ? " compact" : singleLine ? " single-line" : "";
    const row = element("article", `quota-row${quota.status === "stale" ? " stale" : ""}${modeClass}`);
    const title = element("div", "row-title");
    title.append(providerIcon(quota.provider), element("strong", "", label));
    const tone = quota.status === "stale"
      ? "warning"
      : quota.remainingPct >= 50 ? "healthy" : quota.remainingPct >= 20 ? "warning" : "critical";
    if (singleLine) {
      const track = element("div", "quota-track");
      const fill = element("div", "quota-fill");
      fill.style.width = `${Math.max(0, Math.min(100, quota.remainingPct))}%`;
      fill.classList.add(tone);
      track.append(fill);
      title.append(
        track,
        element("span", "section-meta", `${Math.round(quota.remainingPct)}%`),
        element("span", `quota-state-dot ${tone}`),
      );
      row.append(title);
      row.title = quota.status === "stale"
        ? `${label} · 保留上次有效值`
        : `${label} · 剩余 ${Math.round(quota.remainingPct)}% · 重置来源 ${agentDetail.quotaResetSourceLabel(quota)}`;
      ui.quotaList.append(row);
      continue;
    }
    if (compact) {
      title.append(element(
        "span",
        `quota-status-chip ${quota.status === "stale" ? "warning" : "healthy"}`,
        quota.status === "stale" ? "已过期" : "可用",
      ));
      row.append(title);

      const detail = element("div", "quota-compact-detail");
      detail.append(element("strong", "quota-compact-remaining", `剩余 ${Math.round(quota.remainingPct)}%`));
      const track = element("div", "quota-track");
      const fill = element("div", "quota-fill");
      fill.style.width = `${Math.max(0, Math.min(100, quota.remainingPct))}%`;
      fill.classList.add(tone);
      track.append(fill);
      detail.append(track);

      const reset = element(
        "span",
        "quota-compact-reset",
        quota.status === "stale" ? "数据已过期" : `重置时间 · ${agentDetail.quotaResetSourceLabel(quota)}`,
      );
      if (quota.resetsAt) {
        reset.dataset.quotaResetsAt = String(Number(quota.resetsAt) * 1000);
      }
      detail.append(reset);
      if (quota.resetsAt) detail.append(element("span", "quota-source", agentDetail.quotaResetSourceLabel(quota)));
      row.append(detail);
      ui.quotaList.append(row);
      continue;
    }
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
      codex_app_server: "Codex 自动同步",
      statusline: "Claude 对话同步",
      rollout_experimental: "本机 Session 同步",
    }[quota.source];
    if (sourceLabel) meta.append(element("span", "quota-source", sourceLabel));
    if (quota.planType) meta.append(rawElement("span", "", quota.planType));
    if (quota.resetsAt) {
      const reset = new Date(Number(quota.resetsAt) * 1000);
      const resetNode = element("span", "");
      resetNode.dataset.quotaResetsAt = String(reset.getTime());
      meta.append(resetNode);
    }
    meta.append(element("span", "quota-source", `重置来源 · ${agentDetail.quotaResetSourceLabel(quota)}`));
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
  if (settingsState.soundEnabled) window.ActRealmNotificationSound?.play();
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
  window.TokenDecision?.render(snapshot.tokenDecision, settingsState);
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
  } catch (_) {}
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

async function initializeAuthenticatedSession() {
  const hashToken = new URLSearchParams(location.hash.slice(1)).get("bootstrap");
  const plan = I18N.bootstrapPlan({ hashToken, csrfToken });
  if (plan === "bootstrap-first") await bootstrap();
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
  setClientText(ui.runtimeLabel, connected ? "Live · 本地" : "正在重连");
  setClientText(ui.runtimeFooterLabel, connected ? "Runtime · 本机在线" : "Runtime · 正在重连");
  setClientText(ui.runtimeSettingsLabel, connected ? "本机 Runtime 在线" : "本机 Runtime 正在重连");
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

async function connectSocket() {
  if (!csrfToken || restartInProgress) return;
  if (socket && [WebSocket.CONNECTING, WebSocket.OPEN].includes(socket.readyState)) return;
  if (socketTicketInFlight) return;
  window.clearTimeout(reconnectTimer);
  reconnectTimer = undefined;
  socketTicketInFlight = true;
  let ticket;
  try {
    const response = await api("/api/v1/ws-ticket", { method: "POST" });
    ticket = response.ticket;
  } catch (_) {
    setConnected(false);
    if (!restartInProgress) {
      reconnectTimer = window.setTimeout(connectSocket, reconnectDelay);
      reconnectDelay = Math.min(reconnectDelay * 2, 10000);
    }
    return;
  } finally {
    socketTicketInFlight = false;
  }
  if (!ticket || restartInProgress) return;
  if (socket && [WebSocket.CONNECTING, WebSocket.OPEN].includes(socket.readyState)) return;
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const currentSocket = new WebSocket(
    `${scheme}://${location.host}/api/v1/ws`,
    `actrealm.${ticket}`,
  );
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
    const message = error.message === "STALE_APPROVAL"
      ? "这项请求已过期，已交回原终端"
      : `操作失败：${apiErrorText(error)}`;
    showToast(message);
    await loadSnapshot().catch(() => {});
  }
}

function showUndo(commandId, action) {
  undoCommandId = commandId;
  setClientText(ui.undoMessage, `${action === "approve" ? "批准" : "拒绝"} · 3 秒后提交`);
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
    const message = error.message === "STALE_APPROVAL"
      ? "决定已经提交，不能再撤回"
      : `撤回失败：${apiErrorText(error)}`;
    showToast(message);
  }
}

function inferredToastPriority(message) {
  const normalized = String(message).toLowerCase();
  const errorMarkers = ["失败", "无法", "不能", "未连接", "没有可用", "没有找到", "未找到", "已过期", "请输入", "仍需在", "failed", "unable", "cannot", "not connected", "not found", "expired", "enter "];
  return errorMarkers.some((marker) => normalized.includes(marker)) ? 1 : 0;
}

function showToast(message, priority = inferredToastPriority(message)) {
  if (!ui.toast.hidden && priority < toastPriority) return;
  window.clearTimeout(toastTimer);
  toastPriority = priority;
  setClientText(ui.toast, message);
  ui.toast.classList.toggle("error", priority > 0);
  ui.toast.hidden = false;
  toastTimer = window.setTimeout(() => {
    ui.toast.hidden = true;
    toastPriority = 0;
  }, 3200);
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
    ? currentLocale() === "en"
      ? `${elapsedText(status.hook.lastEventAt)} ago`
      : `${elapsedText(status.hook.lastEventAt)}前`
    : tr("尚无真实事件");
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
    ui.runtimeMonitorGrid.replaceChildren(element("span", "monitor-loading", `监控读取失败：${apiErrorText(error)}`));
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
    } catch (_) {}
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

function setRuntimeActionFeedback(message, error = false) {
  setClientText(ui.runtimeActionFeedback, message || "");
  ui.runtimeActionFeedback.hidden = !message;
  ui.runtimeActionFeedback.classList.toggle("error", error);
}

async function restartRuntime() {
  if (restartInProgress) return;
  const waiting = openItems().filter((item) => item.kind === "approval" || item.kind === "native_approval").length;
  if (waiting > 0 && !window.confirm(tr(`当前有 ${waiting} 个请求正在等待。重启会安全放行这些请求并重新连接 Agent。`))) return;
  restartInProgress = true;
  ui.runtimeRestart.disabled = true;
  setClientText(ui.runtimeRestart, "正在保存状态…");
  setRuntimeActionFeedback("正在安全保存状态并重新连接 Runtime…");
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
    setClientText(ui.runtimeLabel, "正在重启");
    setClientText(ui.runtimeFooterLabel, "Runtime · 正在恢复");
    setClientText(ui.runtimeSettingsLabel, "本机 Runtime 正在重启");
    setClientText(ui.runtimeRestart, "正在恢复连接…");
    await waitForRuntimeRestart(previousInstanceId, restartToken);
    restartInProgress = false;
    reconnectDelay = 500;
    connectSocket();
    setConnected(true);
    await loadRuntimeMonitor();
    setRuntimeActionFeedback("Runtime 已重启，Hook 通道与任务状态已恢复。");
  } catch (error) {
    restartInProgress = false;
    setConnected(false);
    setRuntimeActionFeedback(error.message === "RESTART_TIMEOUT"
      ? "Runtime 未能自动恢复，请在终端重新运行 serve --open"
      : `Runtime 重启失败：${apiErrorText(error)}`, true);
  } finally {
    ui.runtimeRestart.disabled = false;
    setClientText(ui.runtimeRestart, "重启 Runtime");
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
ui.settingsSaveRetry.addEventListener("click", () => {
  if (failedSettingsDraft) saveSettings(failedSettingsDraft);
  else loadSettings();
});
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
  settingsState.displayProfile = "custom";
  ui.displayProfile.value = "custom";
  settingsState.taskCardFields = [...ui.taskCardFields.querySelectorAll("input:checked")].map((input) => input.value);
  saveSettings();
});
ui.completionHideMode.addEventListener("change", () => {
  settingsState.completionTaskHideMode = ui.completionHideMode.value;
  renderSettings(); saveSettings();
});
ui.completionAutoHideMinutes.addEventListener("change", () => {
  settingsState.completionAutoHideMinutes = +ui.completionAutoHideMinutes.value; saveSettings();
});
for (const button of ui.quotaDisplayOptions) {
  button.addEventListener("click", () => {
    settingsState.quotaDisplayMode = normalizedQuotaDisplayMode(button.dataset.value);
    renderSettings();
    renderQuota();
    saveSettings();
  });
}
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
ui.clearBackups.addEventListener("click", openBackupClearConfirmation);
ui.backupWipeConfirm.addEventListener("click", clearConfigurationBackups);
ui.backupWipeCancel.addEventListener("click", cancelBackupClearConfirmation);
ui.backupWipeConfirmationInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") clearConfigurationBackups();
});
for (const button of ui.retentionOptions) {
  button.addEventListener("click", () => chooseRetention(Number(button.dataset.value)));
}
ui.runtimeMonitor.addEventListener("click", () => {
  ui.runtimeMonitorDetails.hidden = !ui.runtimeMonitorDetails.hidden;
  setClientText(ui.runtimeMonitor, ui.runtimeMonitorDetails.hidden ? "查看监控" : "收起监控");
  if (!ui.runtimeMonitorDetails.hidden) void loadRuntimeMonitor();
});
ui.runtimeMonitorRefresh.addEventListener("click", loadRuntimeMonitor);
ui.runtimeRestart.addEventListener("click", restartRuntime);
ui.notificationClose.addEventListener("click", () => { ui.notificationBanner.hidden = true; });
ui.notificationView.addEventListener("click", () => {
  const items = openItems();
  const index = items.findIndex((item) => item.id === notificationItemId);
  if (index >= 0) currentAttentionID = items[index].id;
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
  initializeLanguage();
  try {
    await initializeAuthenticatedSession();
    await loadSnapshot();
    await loadSetup();
    await loadSettings();
    await recordUiMetric("app_opened");
    await loadSnapshot();
    knownAttentionIds = new Set(openItems().map((item) => item.id));
    notificationsPrimed = true;
    connectSocket();
  } catch (error) {
    setConnected(false);
    ui.attentionList.replaceChildren(emptyState("!", "无法连接本地 Runtime", "请从 actrealm serve 输出的一次性地址打开控制面板。"));
    showToast(`连接失败：${apiErrorText(error)}`);
  } finally {
    document.body.classList.remove("app-booting");
  }
})();
