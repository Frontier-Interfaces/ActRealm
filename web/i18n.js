"use strict";

(function initializeActRealmI18n(global) {
  const STORAGE_KEY = "actrealm.language";
  const ORIGINAL_TEXT = new WeakMap();
  const ORIGINAL_ATTRIBUTES = new WeakMap();

  const LITERALS_EN = {
  "删除任务": "Delete task",
  "已删除任务 · 收到该会话的新事件后自动显示": "Task removed \u00b7 it will reappear after a new event",
  "仅删除任务卡，不停止 Agent，也不删除原会话和 Token 记录": "Remove only the card; keep the Agent, conversation and usage history",
  "无法保存任务显示设置": "Could not save task visibility settings",
  "语言": "Language",
  "界面语言": "App language",
  "跟随系统": "System Default",
  "简体中文": "Simplified Chinese",
  "系统默认会根据 macOS 首选语言选择中文或英文。": "System Default follows the preferred language in macOS.",
  "通用": "General",
  "Agent": "Agent",
  "settings.tab.agents": "Agents",
  "通知": "Notifications",
  "主题": "Appearance",
  "显示": "Display",
  "数据": "Data",
  "设置": "Settings",
  "设置…": "Settings…",
  "退出 ActRealm": "Quit ActRealm",
  "查看本机服务状态并处理运行问题。": "Check the local service and resolve runtime issues.",
  "本机服务": "Local service",
  "最近同步": "Last sync",
  "尚未同步": "Not synced yet",
  "诊断详情…": "Diagnostics…",
  "重新检查": "Check Again",
  "重启 Runtime": "Restart Runtime",
  "只有诊断详情会显示进程、锁和本机连接等技术信息。": "Process, lock, and local connection details are shown only in Diagnostics.",
  "运行正常": "Running normally",
  "正在启动": "Starting",
  "未连接": "Not connected",
  "正在重启": "Restarting",
  "Agent 事件与本机控制连接可用": "Agent events and local controls are available",
  "正在等待本机服务完成启动": "Waiting for the local service to finish starting",
  "本机服务暂时不可用": "The local service is temporarily unavailable",
  "打开 ActRealm": "Open ActRealm",
  "本机在线": "Local service online",
  "启动中…": "Starting…",
  "Runtime 未连接": "Runtime disconnected",
  "暂无需要处理的事项": "Nothing needs attention",
  "无活动任务": "No active tasks",
  "无活动": "No activity",
  "0% 为完全透明，100% 为完全不透明。上方预览会按主窗口当前比例实时显示最终叠加效果。": "0% is fully transparent and 100% is fully opaque. The preview above shows the final layered appearance at the current main-window scale.",
  "0% 透明": "0% transparent",
  "1 小时未回复将交回 Provider": "Returns control to the Provider after 1 hour without a response",
  "100% 不透明": "100% opaque",
  "12 秒": "12 seconds",
  "180 天": "180 days",
  "20 秒": "20 seconds",
  "24 小时未回复将交回 Provider": "Returns control to the Provider after 24 hours without a response",
  "3 秒内可以撤回 · 尚未写给 Provider": "Undo available for 3 seconds · Not yet sent to the Provider",
  "30 天": "30 days",
  "5 秒": "5 seconds",
  "8 秒": "8 seconds",
  "90 天": "90 days",
  "ActRealm 不在前台时，背景与三栏仍保持当前透明度": "Keep the current background and column opacity when ActRealm is not in front",
  "ActRealm 主窗口跨屏移动后，HUD 会同步跟随": "The HUD follows when the ActRealm window moves to another display",
  "ActRealm 工作区": "ActRealm workspace",
  "ActRealm 工作区尚未就位": "The ActRealm workspace is not ready",
  "ActRealm 通知": "ActRealm Notifications",
  "Agent Focus · 在需要你判断时提醒并带回对应 Agent": "Agent Focus · Alerts you when a decision is needed and brings back the relevant Agent",
  "Agent 事件": "Agent events",
  "Agent 已完成本轮任务，需要你的确认": "The Agent has completed this turn and needs your confirmation",
  "已完成任务": "Completed tasks",
  "只对 Runtime 已确认完成的任务生效": "Applies only after the Runtime verifies completion",
  "已完成任务隐藏方式": "Completed task hiding",
  "确认后隐藏": "After confirmation",
  "自动隐藏": "Automatically",
  "完成后保留": "Keep after completion",
  "运行、授权、提问和报错任务不会超时隐藏": "Running tasks and tasks awaiting approval, answers, or error handling are never hidden by timeout",
  "“知道了”只关闭提醒；到达设定时间才隐藏任务": "Got it only closes the reminder; the task hides at its scheduled time",
  "已完成任务自动隐藏时间": "Completed task auto-hide delay",
  "5 分钟": "5 minutes",
  "15 分钟": "15 minutes",
  "30 分钟": "30 minutes",
  "60 分钟": "60 minutes",
  "Agent 执行出错，或长时间没有进展": "The Agent encountered an error or has made no progress for a while",
  "Agent 接入": "Agent Setup",
  "Agent 提出了需要你回答的问题": "The Agent asked a question that needs your answer",
  "Agent 提问": "Agent question",
  "Agent 正在等待回答。": "The Agent is waiting for an answer.",
  "Agent 绑定工作区": "Bound workspace",
  "Agent 请求执行操作，需要你的批准": "The Agent requested an action that needs your approval",
  "Claude 与 Codex": "Claude and Codex",
  "Claude 对话同步": "Claude conversation sync",
  "Claude 配置无法解析，已停止修改": "Claude configuration could not be parsed, so no changes were made",
  "Claude 额度": "Claude quota",
  "Codex 信任必须在官方界面确认": "Codex trust must be confirmed in the official interface",
  "Codex 同时存在 inline Hook；请先保留一种同层配置形式。": "Codex also has an inline Hook. Keep only one configuration form at this level first.",
  "Codex 启动命令已复制；运行后输入 /hooks": "Codex launch command copied. Run it, then enter /hooks.",
  "Codex 增强活动": "Enhanced Codex activity",
  "GIF 循环": "Looping GIF",
  "HUD 倒计时": "HUD countdown",
  "HUD 胶囊": "HUD capsule",
  "Helper 已进入启动流程，正在等待 Bootstrap 与快照。": "The helper is starting and waiting for bootstrap and a snapshot.",
  "OAuth 自动同步": "Automatic OAuth sync",
  "Provider 数据": "Provider data",
  "Provider 未提供命令预览": "The Provider did not provide a command preview",
  "Provider 未提供更多错误信息。": "The Provider did not provide more error details.",
  "Provider 连接器": "Provider Connector",
  "Runtime · 本机启动中": "Runtime · Starting locally",
  "Runtime · 本机在线": "Runtime · Online locally",
  "Runtime · 本机未连接": "Runtime · Not connected",
  "Runtime 在线": "Runtime online",
  "Runtime 控制通道已断开；此事项仅供查看，恢复连接后才能操作。": "The Runtime control channel is disconnected. This item is view-only until the connection is restored.",
  "Runtime 正在启动": "Runtime starting",
  "Runtime 状态与诊断": "Runtime Status and Diagnostics",
  "Runtime 监控预览": "Runtime monitor preview",
  "app 内没有打包 actrealm Helper，也未配置开发仓库路径": "The app does not contain the ActRealm helper and no development repository path is configured",
  "cargo build --release -p actrealm failed": "Could not build the local Runtime helper",
  "另一个 actrealm 实例已在运行（可先退出终端里的 serve）": "Another ActRealm instance is already running. Stop the terminal serve process first.",
  "发现旧 LaunchAgent：serve --port 已不受当前 Runtime 支持，会反复退出": "An outdated LaunchAgent was found. The current Runtime no longer supports serve --port, so it will repeatedly exit.",
  "已停止参数过期的 com.frontier.actrealm.runtime LaunchAgent": "Stopped the outdated com.frontier.actrealm.runtime LaunchAgent",
  "runtime.error.stop_old": "Could not stop the previous Runtime (PID %@)",
  "runtime.error.replace_abandoned": "Could not safely replace the abandoned Runtime (PID %@)",
  "runtime.error.unrecognized_lock_stop": "runtime.lock is held by unrecognized process PID %@ (%@), so ActRealm did not stop it",
  "runtime.error.unrecognized_lock_takeover": "runtime.lock is held by unrecognized process PID %@ (%@), so ActRealm did not take control",
  "runtime.error.launch": "Could not launch the ActRealm Runtime: %@",
  "runtime.error.exit_status": "ActRealm Runtime exited with status %@",
  "runtime.error.restart_exhausted": "%@; automatic recovery failed 5 times and has stopped retrying",
  "runtime.error.restart_scheduled": "%@; automatically restarting in %@",
  "app-server 已连接；原生审批仍需在 Codex 处理": "app-server connected; native approvals still need to be handled in Codex",
  "app-server 已连接；当前版本审批需原界面": "app-server connected; this version requires approvals in the original interface",
  "· Provider 后续事件已确认继续": "· A later Provider event confirmed continuation",
  "· 尚未写给 Provider": "· Not yet sent to the Provider",
  "· 已写给 Provider，等待后续事件": "· Sent to the Provider, waiting for a later event",
  "「仅进入 ActRealm」无需此行为": "This does not apply to “ActRealm only”",
  "三个预设提供固定字段组合；简洁模式默认显示 7 项关键信息。开启“自定义字段”后可按显示位置逐项调整。原始提示、命令和文件内容不会因此显示。": "The three presets use fixed field sets. Concise shows seven key fields by default. Turn on Custom Fields to adjust each display position. Raw prompts, commands, and file contents are never revealed by this setting.",
  "三栏不透明度": "Three-column opacity",
  "三栏外观": "Column appearance",
  "三种到达策略互斥，选一个": "Choose one of the three mutually exclusive arrival policies",
  "上下文": "Context",
  "不会删除 Hook 接入和配置备份": "Hook setup and configuration backups will not be deleted",
  "不显示聚焦 HUD · 不切换": "No Focus HUD · No switching",
  "不自动切换页面": "Do not switch pages automatically",
  "为保护设置，ActRealm 已拒绝改写；请先恢复或修正配置。": "To protect your setup, ActRealm did not modify it. Restore or fix the configuration first.",
  "主动更新额度": "Refresh quota",
  "主标题": "Primary title",
  "主标题与状态": "Primary title and status",
  "事件保留": "Event retention",
  "事件到达": "Event arrival",
  "事件只进入 ActRealm，不显示聚焦倒计时，也不自动切换页面": "The event enters ActRealm without a Focus countdown or automatic page switching",
  "事件类型": "Event type",
  "事项已进入待处理列表": "The item has been added to Outbox",
  "二次确认后允许": "Allow after confirmation",
  "仅在内存中提交": "Submit in memory only",
  "仅在内存中提交，不写入数据库、日志或导出。": "Submit in memory only, without writing to the database, logs, or exports.",
  "仅进入 ActRealm": "ActRealm only",
  "仅通知": "Notify only",
  "仍在运行，仅可观察": "Still running, view only",
  "从 Agent 打开后开始计算，可选 5 / 10 / 30 秒。": "Starts after opening the Agent. Choose 5, 10, or 30 seconds.",
  "从列表中移除该任务": "Remove this task from the list",
  "任务仍保留在待处理列表": "The task remains in Outbox",
  "任务卡": "Task cards",
  "任务卡位置示意": "Task card position preview",
  "任务卡字段": "Task card fields",
  "任务失败或需要检查": "Task failed or needs review",
  "任务完成": "Task complete",
  "任务摘要": "Task summary",
  "项目未知": "Project unavailable",
  "当前动作": "Current action",
  "当前文件 / 目标": "Current file / target",
  "语义类别与 Provider 工具名": "Semantic category and Provider tool name",
  "仅使用 Provider 明确 path 字段的 basename": "Only a basename from an explicit Provider path field",
  "任务摘要 · 主标题不同时显示在第二行": "Task summary · Shown on the second line when different from the primary title",
  "任务需要处理": "Task needs attention",
  "优先打开具体任务；失败时打开 Agent 页面": "Open the specific task first; fall back to the Agent page if needed",
  "会话累计 Token": "Session tokens",
  "估算 API 价格": "Estimated API price",
  "估算价格": "Estimated cost",
  "使用统计": "Usage statistics",
  "使用背景": "Use background",
  "保持开启": "Keep enabled",
  "保持系统状态": "Keep system state",
  "修复": "Repair",
  "修复二进制": "Repair binary",
  "倒计时结束后才会写给 Provider": "The decision is sent to the Provider only after the countdown",
  "倒计时结束后自动聚焦 · 也可以稍后处理": "Focus automatically when the countdown ends · Or handle it later",
  "允许": "Allow",
  "允许智能聚焦启用台前调度": "Allow Agent Focus to enable Stage Manager",
  "允许触发的事件": "Events that can trigger",
  "允许（3 秒内可撤回）": "Allow (undo within 3 seconds)",
  "先显示 HUD；可立即查看、稍后处理，或在倒计时后自动打开。": "Show the HUD first. View now, handle later, or open automatically after the countdown.",
  "先记录进入前状态；原本开启时保持开启，原本关闭时才临时开启。": "Record the existing state first. Leave it on if already enabled, or enable it temporarily only when needed.",
  "先选择 Agent 绑定工作区": "Choose a bound workspace first",
  "全部处理完毕": "All caught up",
  "关闭": "Off",
  "关闭 Runtime 监控": "Close Runtime Monitor",
  "关闭只影响聚焦，事件仍正常进入 ActRealm": "Turning this off affects Focus only; events still enter ActRealm",
  "关闭后事件仍进入 ActRealm，但不显示聚焦倒计时，也不自动切换 Agent。": "Events still enter ActRealm when off, but no Focus countdown is shown and the Agent is not switched automatically.",
  "关闭后仍保留审批与必要生命周期事件": "Approvals and essential lifecycle events are still retained when off",
  "关闭某项后，此类事件仍会保留在任务记录中，但不会出现在 Outbox。": "When an item is turned off, those events remain in task history but no longer appear in Outbox.",
  "关闭通知": "Dismiss notification",
  "其他工作区": "Other workspace",
  "其他答案（可直接输入）": "Other answer (type directly)",
  "具体任务": "Specific task",
  "决定将在 3 秒撤回窗口后提交": "The decision will be submitted after the 3-second undo window",
  "决定已写给 Provider，等待后续事件确认": "The decision was sent to the Provider and is waiting for a later event",
  "准备中": "Preparing",
  "出错": "Error",
  "出错或卡住": "Error or stuck",
  "切换到放置 Claude、Codex 或终端的工作区后，绑定当前显示器；只有已绑定 Agent 才会触发聚焦。": "Switch to the workspace containing Claude, Codex, or your terminal, then bind the current display. Only bound Agents can trigger Focus.",
  "切换到放置 Claude、Codex 或终端的工作区，再绑定当前显示器上的窗口。": "Switch to the workspace containing Claude, Codex, or your terminal, then bind a window on the current display.",
  "刷新": "Refresh",
  "刷新接入状态": "Refresh setup status",
  "刷新状态": "Refresh status",
  "副标题 · 项目 · 模型 · 计划进度": "Subtitle · Project · Model · Plan progress",
  "副标题与进度": "Subtitle and progress",
  "单行": "Single Line",
  "历史已恢复，等待新事件": "History restored, waiting for new events",
  "原界面请求": "Original-interface request",
  "去 Agent 回答": "Answer in Agent",
  "去核对": "Review",
  "发现不完整或被修改的 ActRealm 条目；不会自动覆盖。": "An incomplete or modified ActRealm entry was found and will not be overwritten automatically.",
  "发现旧的后台启动项": "Legacy background launch item found",
  "发送回答": "Send Answer",
  "取消": "Cancel",
  "取消请求": "Cancel request",
  "只保存事件，不显示聚焦倒计时，也不自动切换页面。": "Save the event without showing a Focus countdown or switching pages automatically.",
  "只展示真实能力；配置冲突时停止写入，不会静默覆盖。": "Only verified capabilities are shown. Configuration conflicts stop writes instead of being overwritten silently.",
  "只恢复本次智能聚焦主动改变的状态": "Restore only state changed by this Focus action",
  "只显示 Runtime 已验证的工具与计划事件": "Show only tools and plan events verified by the Runtime",
  "可用": "Available",
  "可直接允许或拒绝 · 事项保留在待处理列表": "Allow or deny directly · The item remains in Outbox",
  "同时调整 OUTBOX、AGENT TASKS 与 QUOTA": "Adjust OUTBOX, AGENT TASKS, and QUOTA together",
  "否": "No",
  "启用智能聚焦": "Enable Agent Focus",
  "固定显示在下方选择的显示器": "Always show on the display selected below",
  "图片无法解码，请选择 PNG、JPEG、HEIC 或 GIF。": "The image could not be decoded. Choose a PNG, JPEG, HEIC, or GIF.",
  "在 ActRealm 回答": "Answer in ActRealm",
  "在一处管理所有 Agent": "Manage all Agents in one place",
  "在终端运行卡片中的内置 Codex 命令": "Run the built-in Codex command shown on the card in Terminal",
  "声音": "Sound",
  "复制信任命令": "Copy trust command",
  "外部 Hook，仅观察 / 授权": "External Hook, observe / authorize only",
  "失焦时保持透明度": "Preserve opacity when unfocused",
  "失败": "Failed",
  "好": "OK",
  "始终显示在 macOS 当前的主显示器": "Always show on the current macOS main display",
  "子 Agent": "Subagent",
  "存在": "Present",
  "安全接入": "Secure setup",
  "安全接入、真实事件验证和 Codex 信任检查都在这里完成。": "Secure setup, real-event verification, and Codex trust checks are managed here.",
  "安全接入并产生真实会话后读取可验证额度": "Connect securely and start a real session to read verifiable quota data",
  "安装说明…": "Setup Instructions…",
  "完成": "Complete",
  "完成一次 Agent 对话后会同步可验证额度": "Verifiable quota data syncs after an Agent conversation completes",
  "完整": "Full",
  "完整保留全部信息；紧凑使用双行；单行把核心额度排在一行": "Full preserves all information, Compact uses two lines, and Single Line keeps core quota data on one line",
  "实时状态": "Live status",
  "导出使用统计…": "Export Usage Statistics…",
  "导出全部数据…": "Export All Data…",
  "尚未建立": "Not established",
  "尚未生成配置路径": "Configuration path not available yet",
  "尚未绑定": "Not bound",
  "尚未连接任何 Agent": "No Agents connected",
  "展开详情": "Expand details",
  "展开详情中的来源与内部标识": "Source and internal identifier in expanded details",
  "工作区": "Workspace",
  "工作区背景": "Workspace background",
  "已写给 Provider，等待确认": "Sent to the Provider, waiting for confirmation",
  "已在原界面处理": "Handled in the original interface",
  "已处理": "Handled",
  "已失去控制": "Control lost",
  "已开启；支持自动同步和主动更新": "Enabled; supports automatic sync and manual refresh",
  "已接入": "Connected",
  "已收到安装后的真实 Agent 事件，实时活动可以正常显示。": "A real Agent event has been received since setup. Live activity is working.",
  "已结束": "Ended",
  "已解决或稍后处理 → 取消": "Resolved or handle later → Cancel",
  "已过期": "Expired",
  "已连接": "Connected",
  "已重新连接，可控制": "Reconnected, controls available",
  "平均响应": "Average response",
  "开发者": "Developer",
  "开发者信息": "Developer information",
  "开启": "On",
  "开启后可逐项编辑下方任务卡字段": "Turn on to edit each task-card field below",
  "开启自动返回": "Enable automatic return",
  "当前 Provider 版本暂不支持额度解析": "This Provider version does not support quota parsing yet",
  "当前位于其他工作区": "Currently in another workspace",
  "当前工具": "Current tool",
  "当前已连接": "Connected",
  "当前未接收：鼠标仍在其他工作区 · 倒计时结束后返回 ActRealm": "Not received: the pointer is still in another workspace · Return to ActRealm when the countdown ends",
  "当前正在查看智能聚焦（Agent Focus）": "Currently viewing Agent Focus",
  "当前没有可用的直接回复通道；ActRealm 不会把复制文字伪装成已回答。": "No direct reply channel is currently available. ActRealm will not present copied text as an answered question.",
  "当前环境不支持": "Not supported in the current environment",
  "当前规则预览": "Current rule preview",
  "当前：待绑定桌面": "Current: workspace not bound",
  "当前：智能聚焦已关闭": "Current: Agent Focus is off",
  "当前：运行正常": "Current: running normally",
  "彻底清除…": "Erase Everything…",
  "彻底清除运行数据": "Erase all runtime data",
  "循环视频": "Looping video",
  "必填": "Required",
  "恢复状态": "Recovery status",
  "恢复进入前状态的时机": "When to restore the previous state",
  "恢复默认": "Restore Defaults",
  "所选显示器当前工作区的应用": "Apps in the selected display's current workspace",
  "所选显示器未连接时暂用系统主显示器": "Use the system main display temporarily if the selected display disconnects",
  "手动查看": "View manually",
  "打开 ActRealm 设置": "Open ActRealm Settings",
  "打开 Agent 接入中心": "Open Agent Setup",
  "打开 Codex，输入 /hooks，逐项检查并信任 ActRealm。": "Open Codex, enter /hooks, then review and trust ActRealm for each item.",
  "打开任意 Codex 终端会话": "Open any Codex terminal session",
  "打开应用": "Open App",
  "打开智能聚焦（Agent Focus）": "Open Agent Focus",
  "打开设置检查本机服务": "Open Settings to check the local service",
  "托管能力": "Managed capabilities",
  "托管请求已接入，可直接审批": "Managed request connected; direct approval is available",
  "执行计划": "Execution plan",
  "折叠任务卡中的用量胶囊": "Collapse the usage capsule in task cards",
  "折叠任务卡的第一、二行": "Collapse the first and second task-card rows",
  "折叠任务卡的身份信息与计划": "Collapse task identity and plan details",
  "拒绝": "Deny",
  "拒绝提供": "Decline to answer",
  "指定显示器": "Selected display",
  "指定显示器断开时会暂时回退到系统主显示器；审批按钮始终保留。": "If the selected display disconnects, the HUD temporarily falls back to the system main display. Approval controls always remain available.",
  "接入状态由本机实时检测；配置写入前自动备份，不需要 ActRealm 账号。": "Setup status is detected locally in real time. Configuration is backed up before writing, and no ActRealm account is required.",
  "接入状态确认前不会显示伪造的任务或额度": "No fabricated tasks or quota data are shown before setup is verified",
  "接入状态确认前不会显示缓存或演示数据": "No cached or demo data is shown before setup is verified",
  "接收等待时间": "Acceptance wait time",
  "控制任务卡和额度卡的信息密度；只显示 Runtime 允许的安全字段。": "Control the information density of task and quota cards. Only safe fields allowed by the Runtime are shown.",
  "控制连接": "Control connection",
  "控制连接可用，Hook 事件可以进入主界面。": "The control connection is available and Hook events can enter the main interface.",
  "推理 Token": "Reasoning tokens",
  "推荐": "Recommended",
  "提示音": "Alert sound",
  "提醒后聚焦": "Focus after alert",
  "提醒时长": "Alert duration",
  "撤回": "Undo",
  "操作": "Action",
  "支持静态图片、GIF、MP4、MOV 等 macOS 可读取格式。GIF 与视频会静音自动循环；文件只保存在本机。": "Supports still images, GIF, MP4, MOV, and other formats readable by macOS. GIFs and videos loop silently, and files remain on this Mac.",
  "收到允许触发的事件后，直接打开对应 Agent 的具体任务。": "Open the relevant Agent task directly when an allowed event arrives.",
  "数据已过期": "Data expired",
  "新事件会先以 HUD 胶囊出现": "New events first appear in a HUD capsule",
  "新事件到达时在目标显示器的安全区域顶部居中": "Center new events at the top of the target display's safe area",
  "新事件进入 Outbox 时播放本机轻提示音": "Play a subtle local sound when a new event enters Outbox",
  "无人持有": "No owner",
  "无法使用这张图片": "This image cannot be used",
  "无法恢复台前调度进入前状态，请在控制中心检查": "The previous Stage Manager state could not be restored. Check Control Center.",
  "无法更改 macOS 台前调度；仍继续聚焦 Agent": "macOS Stage Manager could not be changed. Agent Focus will continue.",
  "是": "Yes",
  "显示 HUD 胶囊": "Show HUD capsule",
  "显示位置": "Position",
  "显示字段": "Display fields",
  "显示时间": "Display duration",
  "显示档位": "Display mode",
  "智能聚焦": "Agent Focus",
  "智能聚焦 HUD 的等待时间由 Agent Focus 单独设置": "The Agent Focus HUD wait time is configured separately in Agent Focus",
  "智能聚焦只会处理已绑定的 Agent；鼠标进入该工作区即视为已接收。": "Agent Focus handles only bound Agents. Moving the pointer into that workspace counts as acceptance.",
  "智能聚焦已关闭：事件照常进入 ActRealm，不显示聚焦倒计时，也不切换页面": "Agent Focus is off: events still enter ActRealm, but no Focus countdown is shown and pages are not switched",
  "智能聚焦已就绪": "Agent Focus is ready",
  "暂不可用": "Temporarily unavailable",
  "暂无数据": "No data",
  "暂时没有额度数据": "No quota data yet",
  "更新时间未提供": "Update time unavailable",
  "最后事件": "Last event",
  "计划事实": "Plan evidence",
  "活动事实": "Activity evidence",
  "目标事实": "Target evidence",
  "完成事实": "Completion evidence",
  "控制事实": "Control evidence",
  "最近检查": "Last checked",
  "最近没有新的活动": "No recent activity",
  "服务启动中": "Service starting",
  "服务未连接": "Service disconnected",
  "未开启": "Off",
  "未找到": "Not found",
  "未找到客户端": "Client not found",
  "未找到对应 Agent 窗口；事件仍保留在 ActRealm": "No matching Agent window was found. The event remains in ActRealm.",
  "未接入": "Not connected",
  "未接收 → 保持 Agent 页面": "Not received → Stay on Agent page",
  "未接收 → 返回 ActRealm": "Not received → Return to ActRealm",
  "未接收时自动返回": "Return automatically if not received",
  "未提供计划事件": "No plan events provided",
  "未检测到可绑定的应用窗口": "No bindable app window detected",
  "未检测到接收，已返回 ActRealm": "No acceptance detected; returned to ActRealm",
  "未运行": "Not running",
  "未连接 Agent": "Agent not connected",
  "本地数据": "Local data",
  "本地数据已导出": "Local data exported",
  "保存失败：%@": "Could not save: %@",
  "本机 Session 同步": "Local session sync",
  "本机 · 不发送遥测": "Local · No telemetry",
  "本轮 Token": "Turn tokens",
  "本轮上下文": "Turn context",
  "本轮修改已完成，等待确认。": "Changes for this turn are complete and waiting for confirmation.",
  "本轮完成、等待确认": "Turn complete, waiting for confirmation",
  "权限模式": "Permission mode",
  "查看安装说明": "View Setup Instructions",
  "查看待处理事项": "View Outbox",
  "查看接入指南": "View Setup Guide",
  "标记已处理": "Mark Handled",
  "标记已解决": "Mark Resolved",
  "标题来源": "Title source",
  "检查后重新安装": "Review and reinstall",
  "检查设置": "Check Settings",
  "检测到自定义状态栏；开启时会保留原显示": "A custom status bar was detected and will be preserved when enabled",
  "模型未知": "Unknown model",
  "正在使用默认玻璃背景": "Using the default glass background",
  "正在建立撤回窗口…": "Creating undo window…",
  "正在提交允许…": "Submitting approval…",
  "正在提交拒绝…": "Submitting denial…",
  "正在更新…": "Updating…",
  "正在检测 Agent": "Detecting Agents",
  "正在检测本机 Agent": "Detecting local Agents",
  "正在等待 Runtime": "Waiting for Runtime",
  "正在读取可用字段…": "Loading available fields…",
  "正在读取接入状态…": "Loading setup status…",
  "正在读取本机 Agent 接入状态…": "Loading local Agent setup status…",
  "正在进行的任务 · 点击展开详情": "Task in progress · Click to expand details",
  "正在重启 Runtime": "Restarting Runtime",
  "正在重启…": "Restarting…",
  "正式运行时会显示真实进程、锁、Bridge 与连接状态。": "In normal operation, this shows the real process, lock, Bridge, and connection status.",
  "此操作不可撤销。": "This action cannot be undone.",
  "此请求由 Provider 原界面拥有；ActRealm 只同步等待与解决状态。": "This request is owned by the Provider's original interface. ActRealm only syncs its waiting and resolution state.",
  "永久": "Forever",
  "没有可用的 Runtime 控制连接。": "No Runtime control connection is available.",
  "没有正在进行的任务": "No tasks in progress",
  "活跃天数": "Active days",
  "测试智能聚焦": "Test Agent Focus",
  "测试胶囊": "Test Capsule",
  "清除": "Clear",
  "清除数据": "Clear Data",
  "清除绑定": "Clear Binding",
  "点击任务卡后显示": "Shown after clicking a task card",
  "点击任务后展开详细信息与开发者信息": "Click a task to expand details and developer information",
  "点击后先备份，再语义合并；不会静默替换现有配置。": "Clicking first creates a backup, then merges semantically without silently replacing existing configuration.",
  "状态": "Status",
  "状态暂时不可用": "Status temporarily unavailable",
  "状态粒度由当前 Hook / Connector 能力决定": "Status detail depends on current Hook / Connector capabilities",
  "用户接收后恢复": "Restore after user acceptance",
  "用量概览": "Usage overview",
  "由连接器定义": "Defined by Connector",
  "界面更新 p95": "UI update p95",
  "目标 Agent 当前未运行": "The target Agent is not running",
  "目标显示器": "Target display",
  "相关文件缺失，可以安全修复": "Required files are missing and can be repaired safely",
  "确认允许": "Confirm Allow",
  "确认允许运行这条命令？": "Allow this command to run?",
  "确认后归档本轮": "Archive this turn after confirmation",
  "确认完成": "Confirm Complete",
  "知道了": "Got it",
  "确认彻底清除": "Confirm Erase Everything",
  "确认清除": "Confirm Clear",
  "移除": "Remove",
  "移除接入": "Remove Setup",
  "稍后处理": "Handle Later",
  "稍后提醒": "Remind Me Later",
  "空闲": "Idle",
  "窗口失焦": "Window unfocused",
  "立即更新": "Update Now",
  "立即查看": "View Now",
  "立即聚焦": "Focus Now",
  "等待": "Waiting",
  "等待 Runtime 输出…": "Waiting for Runtime output…",
  "等待信任": "Waiting for trust",
  "等待回答": "Waiting for answer",
  "等待处理": "Waiting for action",
  "等待手动查看": "Waiting for manual review",
  "等待批准": "Waiting for approval",
  "等待接收": "Waiting for acceptance",
  "等待时间": "Wait time",
  "等待确认": "Waiting for confirmation",
  "等待确认状态": "Waiting for confirmation status",
  "等待验证": "Waiting for verification",
  "简洁": "Concise",
  "管理 Agent": "Manage Agents",
  "管理 Claude Code、Codex 及可选的本机数据来源。": "Manage Claude Code, Codex, and optional local data sources.",
  "管理本机保留、导出和使用统计；ActRealm 不发送遥测。": "Manage local retention, exports, and usage statistics. ActRealm sends no telemetry.",
  "管理进程": "Managed process",
  "系统主显示器": "System main display",
  "紧凑": "Compact",
  "绑定工作区可见，等待鼠标进入": "Bound workspace visible, waiting for the pointer",
  "绑定当前桌面": "Bind Current Workspace",
  "绑定放置协作应用的虚拟桌面": "Bind the virtual workspace containing collaboration apps",
  "统计只在这台 Mac 上累计。": "Statistics are accumulated only on this Mac.",
  "统计已导出": "Statistics exported",
  "缓存读取 / 写入": "Cache reads / writes",
  "缺失": "Missing",
  "聚焦具体任务": "Focus specific task",
  "聚焦方式": "Focus behavior",
  "聚焦时使用 macOS 台前调度": "Use macOS Stage Manager while focusing",
  "背景图片": "Background image",
  "自定义字段": "Custom fields",
  "视频循环 · 静音": "Looping video · Muted",
  "视频无法播放，请选择 MP4、MOV 或其他 macOS 支持的视频。": "The video could not be played. Choose an MP4, MOV, or another format supported by macOS.",
  "触发智能聚焦的事件": "Events that trigger Agent Focus",
  "计划": "Plan",
  "记录系统状态": "Record system state",
  "识别显示器": "Identify Displays",
  "详细": "Detailed",
  "请先安装该 Agent 的桌面客户端或命令行程序。": "Install this Agent's desktop app or command-line tool first.",
  "请求状态已更新": "Request status updated",
  "请求运行命令或执行操作": "Request to run a command or perform an action",
  "请输入回答。": "Enter an answer.",
  "请输入有效数字。": "Enter a valid number.",
  "请选择": "Choose",
  "请选择“是”或“否”。": "Choose Yes or No.",
  "请选择一个答案。": "Choose an answer.",
  "请选择另一张图片。": "Choose another image.",
  "调整工作区背景、三栏透明度与窗口失焦外观。": "Adjust the workspace background, three-column opacity, and unfocused-window appearance.",
  "超时交还率": "Timeout return rate",
  "超时未检测到鼠标进入绑定工作区时，返回 ActRealm；事件仍保留。": "Return to ActRealm if the pointer does not enter the bound workspace before timeout. The event is retained.",
  "超过保留期的本机事件会自动清理": "Local events older than the retention period are deleted automatically",
  "跟随 ActRealm 窗口": "Follow ActRealm window",
  "输入 / 输出": "Input / output",
  "输入 DELETE；Agent 接入和备份不会被删除": "Enter DELETE. Agent setup and backups will not be deleted",
  "输入回答": "Enter answer",
  "输入数字": "Enter number",
  "运行中": "Running",
  "运行中的子 Agent": "Running subagents",
  "返回 ActRealm 工作区": "Return to ActRealm workspace",
  "返回 ActRealm 时恢复": "Restore when returning to ActRealm",
  "返回 Agent 原窗口": "Return to the original Agent window",
  "还没有需要处理的事项": "Nothing needs attention yet",
  "进入 ActRealm": "Enter ActRealm",
  "进入 Outbox 的事件": "Events in Outbox",
  "进入即视为已接收；事件仍等待批准、回答或确认": "Entering counts as acceptance; the event still waits for approval, an answer, or confirmation",
  "进程存在不代表服务可用；这里同时检查连接、锁和 Bridge。": "A running process does not guarantee service availability. Connection, lock, and Bridge are checked together.",
  "连接 Agent 后，审批、提问和完成确认会出现在这里": "Approvals, questions, and completion confirmations appear here after an Agent is connected",
  "连接 Claude 或 Codex 后，运行中的任务与待处理事项会显示在这里。数据仅留在本机。": "Connect Claude or Codex to see running tasks and items needing attention. Data stays on this Mac.",
  "连接托管": "Managed connection",
  "选择 ActRealm 背景": "Choose ActRealm Background",
  "选择 Agent 绑定工作区": "Choose Bound Workspace",
  "选择哪些事件需要出现在 Outbox 中，提醒你处理。": "Choose which events appear in Outbox to remind you to act.",
  "选择图片 / GIF / 视频…": "Choose Image / GIF / Video…",
  "选择绑定工作区…": "Choose Workspace…",
  "配置": "Configuration",
  "配置写入前会自动备份；Codex Hook 信任需在官方界面确认。": "Configuration is backed up before writing. Codex Hook trust must be confirmed in the official interface.",
  "配置冲突": "Configuration conflict",
  "配置已经就绪；启动一次真实会话后才能确认接入。": "Configuration is ready. Start a real session to verify the connection.",
  "配置无法解析": "Configuration cannot be parsed",
  "配置有变化": "Configuration changed",
  "重启会先校验 runtime.lock 持有者路径，再停止旧进程。": "Restart first verifies the runtime.lock owner's path, then stops the old process.",
  "重新安装": "Reinstall",
  "重新检测": "Detect Again",
  "重新选择…": "Choose Again…",
  "重置时间未提供": "Reset time unavailable",
  "重试": "Retry",
  "锁持有者": "Lock owner",
  "问题": "Question",
  "随上面的选择实时更新": "Updates live with the choices above",
  "需要处理": "Needs attention",
  "需要用户回答的问题": "Question that needs a user response",
  "静态图片": "Still image",
  "面板处理率": "Panel handling rate",
  "面板批准 / 拒绝": "Panel approvals / denials",
  "项目": "Project",
  "预览 HUD": "Preview HUD",
  "额度余量": "Quota remaining",
  "额度卡片": "Quota cards",
  "额度数据已过期": "Quota data expired",
  "额度显示": "Quota display",
  "额度长时间不变或电脑唤醒后可立即请求；若凭证不可用，请先启动 Claude Code CLI 并开始一次会话": "Refresh when quota has not changed for a while or after the Mac wakes. If credentials are unavailable, start Claude Code CLI and begin a session first.",
  "默认使用完整模式。三种模式都会随额度栏宽度自适应，且不改变额度数据与刷新规则。": "Full is the default. All three modes adapt to the quota column width without changing quota data or refresh behavior.",
  "默认开启。关闭后恢复 macOS 原生玻璃行为，窗口失焦时材质会自动变厚。": "On by default. Turning it off restores native macOS glass behavior, which becomes denser when the window loses focus.",
  "默认背景": "Default background",
  "鼠标": "Pointer",
  "鼠标仍在其他工作区 → 倒计时结束后按设置返回 ActRealm": "Pointer still in another workspace → Return to ActRealm after the countdown, as configured",
  "鼠标已进入绑定工作区": "Pointer entered the bound workspace",
  "鼠标进入 Agent 绑定工作区 → 视为已接收，停止自动返回": "Pointer enters the bound workspace → Count as accepted and stop automatic return",
  "鼠标进入绑定工作区即视为已接收": "Moving the pointer into the bound workspace counts as acceptance",
  "鼠标进入绑定工作区即视为已接收，不要求点击或键盘输入": "Moving the pointer into the bound workspace counts as acceptance; no click or keyboard input is required",
  "＋ 连接 Agent": "＋ Connect Agent",
  "当前：队列中 %lld 项": "Current: %lld queued",
  "%lld 个已接入 · %lld 待处理": "%lld connected · %lld need attention",
  "%lld 项接入待处理": "%lld setup items need attention",
  "%lld 项待处理": "%lld waiting",
  "%lld 项出错": "%lld errors",
  "%lld 项运行中": "%lld running",
  "%lld 个任务 · %lld 等待 · %lld 运行中 · %lld 已完成": "%lld tasks · %lld waiting · %lld running · %lld complete",
  "%lld 个任务 · %lld 等待": "%lld tasks · %lld waiting",
  "计划 %lld/%lld": "Plan %lld/%lld",
  "清除任务并安全交还 %lld 项待处理事项": "Clear task and safely return %lld Outbox items",
  "累计 %@ Token": "%@ tokens total",
  "上下文 %lld%%": "Context %lld%%",
  "估算 API 价格 %@": "Estimated API price %@",
  "%lld/%lld（进行中）": "%lld/%lld (in progress)",
  "%@ · 已等 %@": "%@ · Waiting %@",
  "运行失败 · %@": "Failed · %@",
  "本轮已完成 · %@": "Turn complete · %@",
  "最近活动 · %@": "Last active · %@",
  "本轮 %@ · 当前阶段 %@": "Turn %@ · Current phase %@",
  "本轮 %@": "Turn %@",
  "最近同步 · %@": "Last sync · %@",
  "剩余 %lld%%": "%lld%% remaining",
  "%lld 分钟前更新": "Updated %lld min ago",
  "上次记录剩余 %lld%%": "%lld%% remaining at last capture",
  "%@，剩余 %lld%%，%@": "%@: %lld%% remaining, %@",
  "%@，%@": "%@: %@",
  "今天 %@": "Today · %@",
  "陈旧 PID %lld": "Stale PID %lld",
  "唤醒后恢复失败：%@": "Wake recovery failed: %@",
  "已恢复实时连接并请求额度更新": "Live connection restored and quota refresh requested",
  "Runtime 已启动，但控制连接尚未恢复": "Runtime started, but the control connection has not recovered yet",
  "Runtime 已重新启动并恢复连接": "Runtime restarted and reconnected",
  "Runtime 已恢复实时连接": "Runtime live connection restored",
  "%@ 接入已移除": "%@ setup removed",
  "%@ 配置已安全写入": "%@ configuration written safely",
  "接入操作失败：%@": "Setup operation failed: %@",
  "未知错误": "Unknown error",
  "无法读取本机设置，请检查 Runtime 连接后重试。": "Local settings could not be loaded. Check the Runtime connection and try again.",
  "Codex Hook 已更新，请在 Codex 中运行 /hooks 重新检查信任。": "The Codex Hook was updated. Run /hooks in Codex to review trust again.",
  "设置保存失败：%@": "Could not save settings: %@",
  "Claude 额度桥已关闭，原状态栏已恢复": "Claude quota bridge disabled and the original status line restored",
  "Claude 额度桥已开启，完成一次对话后会显示额度": "Claude quota bridge enabled. Quota appears after a conversation completes.",
  "额度桥操作失败：%@": "Quota bridge operation failed: %@",
  "Runtime 未连接，暂时无法刷新额度": "Runtime is disconnected, so quota cannot be refreshed right now",
  "正在通过 Anthropic 官方接口更新…": "Updating through Anthropic's official API…",
  "额度刷新失败：%@": "Quota refresh failed: %@",
  "Claude 额度已主动更新": "Claude quota refreshed",
  "已请求刷新，但 Claude 暂未返回新额度；请启动 Claude Code CLI 并开始一次会话后重试": "Refresh requested, but Claude has not returned new quota data yet. Start Claude Code CLI, begin a session, and try again.",
  "导出失败：%@": "Export failed: %@",
  "请输入 DELETE；没有删除任何数据": "Enter DELETE. No data was removed.",
  "清除失败：%@": "Clear failed: %@",
  "本地运行数据已彻底清除，Hook 接入保持不变": "Local runtime data was erased. Hook setup is unchanged.",
  "Runtime 控制通道已断开；请恢复连接后再回答": "The Runtime control channel is disconnected. Restore the connection before answering.",
  "这个问题没有可用的回复通道": "This question has no available reply channel",
  "回答失败：%@": "Answer failed: %@",
  "已交回 Agent 原界面回答": "Returned to the Agent's original interface for answering",
  "回答已安全发送给 Agent": "Answer sent safely to the Agent",
  "当前环境不支持跳转；ActRealm 不会假装已定位到原对话": "Jumping is not supported in the current environment. ActRealm will not pretend it located the original conversation.",
  "跳转失败：%@": "Jump failed: %@",
  "没有找到原窗口": "The original window was not found",
  "已定位对应任务；当前没有可验证的原窗口跳转信息": "The matching task was located, but no verifiable original-window jump information is available.",
  "托管连接失败：%@": "Managed connection failed: %@",
  "已连接 ActRealm app-server；Codex 原生窗口仍保留当前 Turn 的控制权": "Connected to ActRealm app-server. The native Codex window still controls the current turn.",
  "处理失败：%@": "Action failed: %@",
  "已定位到 Outbox 待处理事项": "Located the related Outbox item",
  "任务已清除；%lld 项仍需在 Outbox 或 Agent 原界面处理": "Task cleared. %lld items still need attention in Outbox or the Agent's original interface.",
  "任务已清除，并交还 %lld 项待处理事项": "Task cleared and %lld items returned to Outbox",
  "已从列表移除；有新活动时会自动恢复": "Removed from the list. It will reappear when new activity arrives.",
  "当前工作区没有可绑定的 Agent 应用": "No bindable Agent app was found in the current workspace",
  "Agent 的绑定工作区已保存": "Bound workspace saved",
  "Agent 的绑定工作区已清除": "Bound workspace cleared",
  "已稍后处理；事件仍保留在 ActRealm": "Set aside for later. The event remains in ActRealm.",
  "已接收 · 事件仍等待实际处理": "Accepted · The event still awaits the actual action",
  "已在对应 Agent 的绑定工作区 · 不重复切换": "Already in the relevant bound workspace · No additional switch",
  "未找到具体任务或 Agent 页面，已返回 ActRealm": "No specific task or Agent page was found, so ActRealm was restored",
  "事件已进入 ActRealm；对应 Agent 尚未绑定到工作区": "The event entered ActRealm, but the relevant Agent has not been bound to a workspace",
  "事件已进入 ActRealm，不自动切换页面": "The event entered ActRealm without switching pages automatically",
  "未检测到鼠标进入对应 Agent 的绑定工作区，已返回 ActRealm": "The pointer did not enter the relevant bound workspace, so ActRealm was restored",
  "未检测到接收；保持当前 Agent 页面": "No acceptance detected; staying on the current Agent page",
  "已确认任务完成": "Task completion confirmed",
  "已确认任务完成：%@": "Task completion confirmed: %@",
  "已标记问题解决": "Issue marked resolved",
  "已标记问题解决：%@": "Issue marked resolved: %@",
  "已标记提问处理": "Question marked handled",
  "已标记提问处理：%@": "Question marked handled: %@",
  "已确认处理": "Marked handled",
  "已确认处理：%@": "Marked handled: %@",
  "%@ 请求运行 %@，等待批准": "%@ requests to run %@, waiting for approval",
  "%@ 请求一次操作，等待批准": "%@ requests an action, waiting for approval",
  "%@ 等待在原界面批准": "%@ is waiting for approval in the original interface",
  "%@ 发出一个待回答问题": "%@ asked a question",
  "任务运行失败，需要检查": "The task failed and needs review",
  "原界面批准": "Original-interface approval",
  "提问": "Question",
  "在跑": "Running",
  "未命名任务": "Untitled task",
  "本周": "This week",
  "Hook 已安装 · 已验证真实事件": "Hook installed · Real event verified",
  "Hook 已安装 · 等待首个事件": "Hook installed · Waiting for first event",
  "需要在 Codex 里完成信任确认（/hooks）": "Trust must be confirmed in Codex (/hooks)",
  "Hook 需要重新安装": "Hook needs to be reinstalled",
  "未安装 Hook": "Hook not installed",
  "未找到该 Provider 的客户端": "Provider client not found",
  "配置存在冲突，请检查": "Configuration conflict found",
  "配置读取出错": "Configuration read error",
  "检测到桌面客户端与 CLI": "Desktop app and CLI detected",
  "检测到桌面客户端 · 不要求全局 CLI": "Desktop app detected · Global CLI not required",
  "检测到 CLI": "CLI detected",
  "尚未检测到可用客户端": "No available client detected yet",
  "任务标题与摘要": "Task title and summary",
  "主标题；不同的任务摘要显示在下一行": "Primary title; a different task summary appears on the next line",
  "标题栏右侧的运行阶段与耗时": "Current phase and elapsed time on the right side of the title row",
  "副标题中的项目名称": "Project name in the subtitle",
  "模型": "Model",
  "副标题中的模型名称": "Model name in the subtitle",
  "计划进度": "Plan progress",
  "完成步数与进度条": "Completed steps and progress bar",
  "折叠卡用量胶囊": "Usage capsule on the collapsed card",
  "上下文占用": "Context usage",
  "当前上下文百分比": "Current context percentage",
  "估算值，不是订阅账单": "Estimate, not a subscription bill",
  "最近一轮 Token": "Most recent turn tokens",
  "输入 / 输出 Token": "Input / output tokens",
  "输入与输出拆分": "Input and output breakdown",
  "缓存读取 / 写入 Token": "Cache read / write tokens",
  "缓存用量拆分": "Cache usage breakdown",
  "Provider 推理用量": "Provider reasoning usage",
  "运行环境": "Runtime environment",
  "最后事件时间": "Last event time",
  "已等 %@": "Waiting %@",
  "队列 · 还有 %lld 项": "Queue · %lld more",
  "最久等待 %lld 分钟": "Longest wait: %lld min",
  "等待处理 · %lld 分钟后过期": "Waiting · Expires in %lld min",
  "%@（主显示器）": "%@ (Main Display)",
  "%@（未连接）": "%@ (Disconnected)",
  "%@已复制到 ActRealm 的本地应用数据目录": "%@ copied to ActRealm's local app data folder",
  "已接入 %lld": "%lld connected",
  "待处理 %lld": "%lld need attention",
  "1. %@   2. 输入 /hooks   3. 核对命令路径并信任   4. 新建会话后刷新": "1. %@   2. Enter /hooks   3. Review and trust the command path   4. Start a new session and refresh",
  "聚焦方式：%@": "Focus behavior: %@",
  "%lld 秒": "%lld sec",
  "等待鼠标进入 %lld 秒": "Wait for pointer · %lld sec",
  "HUD %lld 秒": "HUD · %lld sec",
  "允许触发的事件会立即聚焦对应 Agent 的具体任务，%lld 秒内鼠标未进入绑定工作区则返回 ActRealm": "Allowed events focus the relevant Agent task immediately. Return to ActRealm if the pointer does not enter the bound workspace within %lld seconds.",
  "允许触发的事件会立即聚焦对应 Agent 的具体任务，不自动返回": "Allowed events focus the relevant Agent task immediately, without returning automatically.",
  "先显示 %lld 秒 HUD，可立即查看或稍后处理；倒计时后聚焦 Agent；%lld 秒未接收则返回 ActRealm": "Show the HUD for %lld seconds. View now or handle later; focus the Agent after the countdown, then return to ActRealm if not accepted within %lld seconds.",
  "先显示 %lld 秒 HUD，可立即查看或稍后处理；倒计时后聚焦 Agent": "Show the HUD for %lld seconds. View now or handle later, then focus the Agent after the countdown.",
  "显示器": "Display",
  "任务：%@": "Task: %@",
  "正在聚焦 %@": "Focusing %@",
  "等待鼠标进入绑定工作区 · %lld 秒": "Waiting for the pointer to enter the bound workspace · %lld sec",
  "%@ 分钟前更新": "Updated %1$@ min ago",
  "上次记录剩余 %@": "%@ remaining at last capture",
  "%@ 后自动重启": "Automatic restart in %@",
  "运行失败": "Failed",
  "本轮已完成": "Turn complete",
  "5 小时": "5 hours",
  "7 天": "7 days",
  "额外用量": "Extra usage",
  "730 小时": "730 hours",
  "session.activity.idle": "Waiting for a new task",
  "session.activity.ended": "Session ended",
  "session.activity.thinking": "Thinking",
  "session.activity.tool_running": "Running {tool}",
  "session.activity.awaiting_approval": "Waiting for your approval",
  "session.activity.awaiting_answer": "Waiting for your answer",
  "session.activity.compacting": "Compacting context",
  "session.activity.completed": "Turn completed",
  "session.activity.interrupted": "Turn interrupted",
  "session.activity.failed": "Run failed",
  "session.activity.plan_progress": "Plan progress {done}/{total}",
  "session.activity.subagents_running": "{count} subagents running",
  "session.activity.background_tasks_running": "{count} background tasks still running",
  "session.activity.permission_denied": "The operation was denied in the Agent",
  "session.activity.waiting_for_provider_event": "Waiting for the Agent's next event",
  "session.activity.unknown_event": "Unrecognized event; the Provider version may be incompatible",
  "attention.approval.title": "Approval required",
  "attention.native_approval.title": "Approve in {provider}",
  "attention.question.title": "{provider} is asking a question",
  "attention.error.title": "Agent run failed",
  "attention.interrupted.title": "Agent turn interrupted",
  "attention.completion.title": "Task completed; waiting for confirmation",
  "attention.approval.detail": "Review the operation and its impact in the original conversation.",
  "attention.native_approval.detail": "Review and handle this request in {provider}.",
  "attention.question.detail": "Answer directly in ActRealm. Answers are not written to local history.",
  "attention.risk.high_impact": "High-impact operation detected",
  "attention.risk.irreversible": "The operation cannot be undone after it is submitted",
  "attention.risk.compound_syntax": "The command contains compound syntax",
  "attention.risk.read_only_intent": "Read-only intent; this rule is not a security guarantee",
  "attention.risk.undo_window": "The approval decision can be undone for 3 seconds",
  "attention.risk.side_effects": "May run project code or produce side effects",
  "attention.risk.unknown_impact": "The impact of this operation is unknown",
  "attention.risk.review_original": "Review the original window",
  "interaction.claude_question.title": "Claude is asking",
  "interaction.claude_elicitation.title": "Claude needs more information",
  "interaction.codex_user_input.title": "Codex is asking",
  "interaction.agent_question.title": "Agent is asking",
  "quota.reason.agent_unavailable": "The Agent did not return account quota information.",
  "quota.reason.agent_refresh_failed": "Agent quota refresh failed; showing the last captured value.",
  "jump.exact_conversation": "Open exact conversation",
  "jump.terminal": "Open terminal",
  "jump.app_only": "Open application",
  "jump.unsupported": "Jump is not supported",
  "quota.window.months": "{count} months",
  "quota.window.weeks": "{count} weeks",
  "quota.reason.cache_stale": "This is a historical quota value; waiting for a fresh Provider update.",
  "quota.window.scoped": "{name} quota",
  "quota.window.scoped_weeks": "{name} \u00b7 {count} weeks",
  "quota.window.claude_weekly": "Weekly \u00b7 all models",
  "quota.window.days": "{count} days",
  "quota.window.hours": "{count} hours",
  "quota.window.minutes": "{count} minutes",
  "quota.window.current_week": "This week",
  "quota.window.extra_usage": "Extra usage",
  "quota.reason.cache_missing": "Quota cache is missing. Enable the Claude quota bridge and complete one conversation.",
  "quota.reason.cache_unreadable": "Quota cache could not be read: {error}",
  "quota.reason.cache_incompatible": "Quota cache schema is incompatible.",
  "quota.reason.cache_invalid": "Quota cache could not be parsed.",
  "quota.reason.cache_from_future": "Quota cache timestamp is later than the local clock.",
  "quota.reason.no_valid_window": "No verifiable quota window was found.",
  "quota.reason.codex_rollout_missing": "No Codex rollout file was found.",
  "quota.reason.codex_window_missing": "No verifiable quota window was found in the Codex rollout.",
  "quota.reason.claude_refresh_failed": "Claude quota refresh failed. The last successfully captured values are shown.",
  "quota.reason.codex_refresh_failed": "Codex quota refresh failed. The last successfully captured values are shown.",
  "额度更新结果：%@": "Quota update result: %@",
  "本机请求失败（HTTP %@）": "Local request failed (HTTP %@)",
  "请求失败（%@）": "Request failed (%@)",
  "请求失败，请重试": "The request failed. Try again.",
  "归档": "Archive",
  "归档不会停止 Provider；只有新 Turn 会让任务重新出现": "Archiving does not stop the Provider. Only a new Turn makes the task reappear.",
  "运行中或等待处理的任务不能归档": "Running tasks and tasks awaiting attention cannot be archived",
  "任务已归档；Provider 未停止，收到新 Turn 后会重新出现": "Task archived. The Provider was not stopped; a new Turn will make it active again.",
  "ANSWER_FAILED": "The answer could not be sent to the Agent",
  "AUTH_UNAVAILABLE": "Runtime authentication is unavailable",
  "BACKUP_CLEAR_FAILED": "Configuration backups could not be cleared safely",
  "BACKUP_DELETE_CONFIRMATION_REQUIRED": "Enter DELETE BACKUPS to clear configuration backups",
  "CLAUDE_BRIDGE_CHANGE_FAILED": "The Claude quota bridge could not be updated",
  "CLAUDE_OAUTH_DISABLED": "The official Claude OAuth quota endpoint is disabled",
  "CLAUDE_SIGN_IN_REQUIRED": "Sign in to Claude Code once, then refresh. No conversation is needed.",
  "CLAUDE_AUTH_REFRESH_FAILED": "Automatic Claude credential renewal failed. Check Claude Code sign-in or try again.",
  "CLAUDE_QUOTA_RATE_LIMITED": "Claude is limiting quota requests. Automatic refresh will retry later.",
  "CLAUDE_QUOTA_REFRESH_FAILED": "Claude quota refresh failed",
  "CLEAR_FAILED": "Local data could not be cleared",
  "CODEX_REINSTALL_FAILED": "The Codex Hook could not be reinstalled",
  "COMMAND_MISMATCH": "The command does not match the current request",
  "COMMIT_TOO_EARLY": "The decision is still inside the undo window",
  "CONNECTOR_ATTACH_FAILED": "The Connector could not be attached",
  "DELETE_CONFIRMATION_REQUIRED": "Enter DELETE to clear local data",
  "EXPORT_FAILED": "Local data could not be exported",
  "INVALID_ACTION": "This action is invalid",
  "INVALID_ANSWER": "The answer is invalid",
  "INVALID_BOOTSTRAP": "The launch credential is invalid or expired",
  "INVALID_COMMAND_ID": "The command identifier is invalid",
  "INVALID_HOST": "The address is not a trusted local host",
  "INVALID_HISTORY_LIMIT": "The history task limit is invalid",
  "INVALID_ORIGIN": "The request origin is not trusted",
  "INVALID_RESTART_TOKEN": "The Runtime restart credential is invalid",
  "INVALID_SETTINGS": "The settings are invalid",
  "CHECKPOINT_NOT_FOUND": "The checkpoint was not found",
  "CHECKPOINT_INVALID": "The checkpoint request is invalid",
  "CHECKPOINT_GIT_FAILED": "The Git checkpoint could not be created or applied safely",
  "CHECKPOINT_PREFLIGHT_FAILED": "Checkpoint recovery preflight did not pass",
  "ARTIFACT_REVEAL_FAILED": "Finder could not reveal the file. Please try again",
  "ARTIFACT_UNAVAILABLE": "The referenced file is unavailable or no longer belongs to this result",
  "JUMP_FAILED": "The original window was not found, or app-control permission is missing",
  "JUMP_UNSUPPORTED": "Jumping is not supported in the current environment",
  "MANAGED_CONNECTOR_UNSUPPORTED": "This session does not support a managed Connector",
  "METRIC_RECORD_FAILED": "The local metric could not be recorded",
  "MISSING_REQUEST_ID": "This request has no replyable request identifier",
  "PROVIDER_CLIENT_MISSING": "The corresponding Agent client was not found",
  "PROVIDER_INSTALL_REQUIRED": "Install the corresponding Agent client first",
  "QUESTION_EXPIRED": "This question has expired and cannot be submitted",
  "QUOTA_PERSIST_FAILED": "The quota result could not be saved locally",
  "QUOTA_REFRESH_FAILED": "Quota refresh failed",
  "QUOTA_REFRESH_IN_PROGRESS": "Quota refresh is already in progress",
  "QUOTA_STATE_UNAVAILABLE": "Quota state is unavailable",
  "REQUEST_MISMATCH": "The request does not match the current attention item",
  "RETENTION_FAILED": "The local retention policy could not be applied",
  "RUNTIME_RESTART_FAILED": "Runtime restart failed",
  "RUNTIME_RESTART_TIMED_OUT": "Runtime restart timed out",
  "RUNTIME_RESTART_UNAVAILABLE": "This Runtime cannot restart automatically",
  "SESSION_NOT_FOUND": "The corresponding task was not found",
  "TASK_STILL_ACTIVE": "This task is still running or awaiting attention and cannot be archived or deleted",
  "SETTINGS_READ_FAILED": "Local settings could not be read",
  "SETUP_CHANGE_FAILED": "Agent setup could not be updated",
  "SETUP_INSPECTION_FAILED": "Agent setup status could not be inspected",
  "STALE_APPROVAL": "This approval request has expired",
  "STALE_ATTENTION": "This attention item has changed or expired",
  "STORAGE_ERROR": "Local storage is unavailable",
  "UNAUTHORIZED": "The local session has expired; reconnect to continue",
  "UNAUTHORIZED_MUTATION": "This request cannot modify local state",
  "UNAUTHORIZED_WEBSOCKET": "Realtime connection authentication failed",
  "UNKNOWN_ACTION": "Unknown action",
  "UNKNOWN_BRIDGE_ACTION": "Unknown quota-bridge action",
  "UNKNOWN_MANAGE_ACTION": "Unknown managed-session action",
  "UNKNOWN_PROVIDER": "Unknown Agent type",
  "UNKNOWN_SETUP_ACTION": "Unknown setup action",
  "UNSAFE_DATA_PATH": "The local data path failed its safety check",
  "RUNTIME_NOT_CONNECTED": "Runtime is not connected",
  "RUNTIME_AUTH_FAILED": "Runtime authentication failed",
  "RUNTIME_SESSION_MISSING": "Runtime did not return a local session",
  "client.agent_focus.test.title": "Agent Focus test"
};

  const EXTRA_EN = {
    "0 个任务": "0 tasks",
    "Agent 任务": "Agent tasks",
    "Hook 事件与审批命令通过本地": "Hook events and approval commands use the local",
    "与 Runtime 失去连接，正在重连；当前显示的是最后一次本机快照。": "Runtime connection lost. Reconnecting; the last local snapshot is shown.",
    "任务详情": "Task details",
    "传递": "Transport",
    "决定将在 3 秒后提交": "The decision will be submitted in 3 seconds",
    "实时读取，不发送遥测": "Read live, with no telemetry",
    "导出 JSON": "Export JSON",
    "导出或清理本机记录": "Export or clear local records",
    "导出统计": "Export metrics",
    "配置备份": "Configuration backups",
    "修改 Agent 配置前创建；不会自动删除": "Created before Agent configuration changes; never deleted automatically",
    "清除配置备份…": "Clear Configuration Backups…",
    "只删除 ActRealm 所有的私有备份；遇到符号链接或陌生文件会拒绝操作。": "Only private backups owned by ActRealm are removed. Symlinks or unknown files make the operation fail safely.",
    "输入 DELETE BACKUPS；不会删除当前 Agent 配置": "Type DELETE BACKUPS. Current Agent configuration will not be deleted.",
    "确认清除备份": "Confirm Backup Deletion",
    "请输入 DELETE BACKUPS；没有删除任何备份": "Enter DELETE BACKUPS. No backups were removed.",
    "ActRealm 配置备份已清除": "ActRealm configuration backups cleared",
    "备份清除失败：": "Backup deletion failed: ",
    "彻底清除": "Clear permanently",
    "彻底清除需要输入": "Permanent clearing requires typing",
    "待处理": "Outbox",
    "所有设置和数据都只留在这台 Mac；ActRealm 不发送遥测。": "All settings and data stay on this Mac. ActRealm sends no telemetry.",
    "打开通知与数据": "Open Notifications and Data",
    "数据仅在这台 Mac": "Data stays on this Mac",
    "新事件进入 Outbox 时播放轻提示音": "Play a soft sound when a new item enters Outbox",
    "最近同步 · --:--:--": "Last sync · --:--:--",
    "本机 Runtime 在线": "Local Runtime online",
    "本机健康监控": "Local health monitor",
    "本机累计 · 不发送遥测": "Local totals · No telemetry",
    "查看": "View",
    "查看监控": "View monitor",
    "正在读取 Runtime 状态…": "Reading Runtime status…",
    "正在连接 Runtime": "Connecting to Runtime",
    "等待回答进入 Outbox": "Put questions in Outbox",
    "等待批准进入 Outbox": "Put approvals in Outbox",
    "等待确认进入 Outbox": "Put completions in Outbox",
    "自定义": "Custom",
    "调整任务与额度信息密度": "Adjust task and quota information density",
    "超过保留期的本机事件自动清理": "Local events are removed after the retention period",
    "跳转": "Jump",
    "输入 DELETE 确认彻底清除本机事件与统计": "Type DELETE to permanently clear local events and metrics",
    "连接 Agent": "Connect Agent",
    "通知与数据": "Notifications and Data",
    "需要处理进入 Outbox": "Put errors in Outbox",
    "额度": "Quota",
    "额度卡片显示模式": "Quota card display mode",
    "；Hook 接入和备份不会被删除。": "; Hook setup and backups are not deleted.",
    "尚未连接任何 Agent": "No Agent connected",
    "连接 Claude 或 Codex 后，运行中的任务与待处理事项会显示在这里。数据仅留在本机。": "Connect Claude or Codex to see active tasks and Outbox items here. Data stays local.",
    "＋ 连接 Agent": "+ Connect Agent",
    "未接入": "Not connected",
    "尚未检测到桌面客户端或 CLI。": "No desktop client or CLI detected.",
    "安全接入并产生真实会话后读取可验证额度。": "Verified quota appears after secure setup and a real session.",
    "数据仅在这台 Mac · 不发送遥测": "Data stays on this Mac · No telemetry",
    "最近结果": "Latest result",
    "不会修改现有配置，点击后先备份再语义合并。": "Existing configuration is preserved; ActRealm backs it up before a semantic merge.",
    "未找到客户端": "Client not found",
    "请先安装这个 Agent 的桌面客户端或命令行程序。": "Install this Agent's desktop client or CLI first.",
    "状态暂时无法识别。": "The status is not currently recognized.",
    "检测到桌面客户端与 CLI": "Desktop client and CLI detected",
    "检测到桌面客户端 · 不要求全局 CLI": "Desktop client detected · Global CLI not required",
    "检测到 CLI": "CLI detected",
    "尚未检测到可用客户端": "No supported client detected",
    "没有检测到可用的 Codex 启动命令，请查看接入指南": "No usable Codex launch command was found. See the setup guide.",
    "Codex 启动命令已复制；请在终端运行后输入 /hooks": "Codex launch command copied. Run it in Terminal, then enter /hooks.",
    "复制失败；请手动复制卡片中的 Codex 启动命令": "Copy failed. Copy the Codex launch command from the card manually.",
    "完成接入": "Finish setup",
    "配置": "Configuration",
    "尚未生成配置路径": "Configuration path not available yet",
    "修复二进制": "Repair helper",
    "安全接入": "Set up securely",
    "检查后重新安装": "Inspect and reinstall",
    "复制信任命令": "Copy trust command",
    "移除接入": "Remove setup",
    "重新检测": "Check again",
    "查看安装说明": "View installation instructions",
    "Codex 信任必须在官方界面确认": "Codex trust must be confirmed in the official interface",
    "打开任意 Codex 终端会话": "Open any Codex terminal session",
    "输入 /hooks": "Enter /hooks",
    "核对命令路径后选择信任": "Verify the command path, then choose Trust",
    "启动一个新会话并回到这里刷新": "Start a new session, then return here and refresh",
    "接入状态读取失败": "Could not read setup status",
    "接入操作失败": "Setup action failed",
    "已开启 · 等待 Claude 下一次响应更新": "Enabled · Waiting for Claude's next response",
    "桥接文件缺失，可安全修复": "Bridge files are missing and can be repaired safely",
    "检测到自定义状态栏；可以保留原显示并串联额度采集": "A custom status line was detected; its display can be preserved while quota collection is chained",
    "当前 Runtime 未启用": "Not enabled in the current Runtime",
    "当前版本不可用": "Unavailable in this version",
    "保留现有并开启": "Preserve existing and enable",
    "设置读取失败": "Could not read settings",
    "设置保存失败": "Could not save settings",
    "仅统计数据已导出，不含会话和事件明细": "Metrics exported without session or event details",
    "统计导出失败": "Metrics export failed",
    "等待处理": "Waiting",
    "决定已发送": "Decision sent",
    "已确认继续": "Continuation confirmed",
    "已解决": "Resolved",
    "已交回终端": "Returned to Terminal",
    "已过期，交回终端": "Expired and returned to Terminal",
    "稍后提醒": "Remind later",
    "已忽略": "Ignored",
    "活跃天数": "Active days",
    "面板批准 / 拒绝": "Panel approvals / denials",
    "面板处理率": "Panel handling rate",
    "超时交还率": "Timeout return rate",
    "平均响应": "Average response",
    "页面渲染 p95": "Page rendering p95",
    "一项工具操作": "a tool action",
    "任务出错停下来了": "The task stopped with an error",
    "这一轮已经完成": "This turn is complete",
    "Agent 有一项待处理事项": "The Agent has an item that needs attention",
    "原界面请求批准": "Approval requested in the original interface",
    "等待原界面处理": "Waiting for the original interface",
    "可在 ActRealm 审批": "Can approve in ActRealm",
    "可在 ActRealm 回答": "Can answer in ActRealm",
    "等待回答": "Waiting for an answer",
    "任务已完成": "Task complete",
    "需要处理 · 任务已暂停": "Needs attention · Task paused",
    "回答已安全发送给 Agent": "Answer sent securely to the Agent",
    "已交回 Agent 原界面回答": "Returned to the Agent's original interface",
    "问题": "Question",
    "其他答案（可直接输入）": "Other answer (type directly)",
    "请选择": "Choose",
    "是": "Yes",
    "否": "No",
    "发送回答": "Send answer",
    "拒绝提供": "Decline to answer",
    "取消请求": "Cancel request",
    "去 Agent 回答": "Answer in Agent",
    "在 Agent 任务中查看 →": "View in Agent tasks →",
    "风险标记": "Risk",
    "未知": "Unknown",
    "返回原窗口": "Return to original window",
    "二次确认后允许": "Allow after confirmation",
    "确认允许运行这项操作？": "Allow this operation to run?",
    "撤回决定": "Undo decision",
    "队列": "Queue",
    "累计": "Total",
    "上下文": "Context",
    "估算 API 价格": "Estimated API price",
    "计划": "Plan",
    "清除": "Clear",
    "查看待处理事项": "View Outbox item",
    "暂不可用": "Temporarily unavailable",
    "检查设置": "Check settings",
    "如何开启": "How to enable",
    "剩余": "Remaining",
    "保留上次有效值": "Showing last valid value",
    "刚刚更新": "Updated just now",
    "最近同步 · 等待 Agent 接入": "Last sync · Waiting for Agent setup",
    "最近同步 · 等待额度来源": "Last sync · Waiting for quota source",
    "在线": "Online",
    "异常": "Issue",
    "正在保存状态…": "Saving state…",
    "正在安全保存状态并重新连接 Runtime…": "Safely saving state and reconnecting Runtime…",
    "正在重启": "Restarting",
    "Runtime · 正在恢复": "Runtime · Recovering",
    "本机 Runtime 正在重启": "Local Runtime restarting",
    "正在恢复连接…": "Restoring connection…",
    "收起监控": "Hide monitor",
    "监控读取失败": "Could not read monitor",
    "Live · 本地": "Live · Local",
    "正在重连": "Reconnecting",
    "Runtime · 正在重连": "Runtime · Reconnecting",
    "本机 Runtime 正在重连": "Local Runtime reconnecting",
    "最近 Hook": "Latest Hook",
    "Hook 通道正常": "Hook channel healthy",
    "Hook 通道不可用": "Hook channel unavailable",
    "Hook 状态未知": "Hook status unknown",
    "Hook 通道缺失": "Hook channel missing",
    "Socket 权限异常": "Socket permissions issue",
    "Socket 类型异常": "Socket type issue",
    "SQLite / 重启": "SQLite / restart",
    "本次启动未重启": "No restart this launch",
    "新的授权、问题、完成或错误会实时进入 OUTBOX。": "New approvals, questions, completions, or errors enter OUTBOX live.",
    "当前没有活跃任务": "No active tasks",
    "等待下一条任务": "Waiting for the next task",
    "只保存在当前浏览器，不会修改 Runtime 设置": "Saved only in this browser; Runtime settings are unchanged",
    "模型未知": "Model unknown",
    "未提供": "Not provided",
    "当前环境不支持": "Not supported in the current environment",
    "运行中": "Running",
    "任务详情": "Task details",
    "批准 · 3 秒后提交": "Approve · Submits in 3 seconds",
    "拒绝 · 3 秒后提交": "Deny · Submits in 3 seconds",
    "操作失败": "Operation failed",
    "撤回失败": "Undo failed",
    "回答失败": "Answer failed",
    "跳转失败": "Jump failed",
    "托管连接失败": "Managed connection failed",
    "Runtime 重启失败": "Runtime restart failed",
    "连接失败": "Connection failed",
    "额度桥操作失败": "Quota bridge action failed",
    "导出失败": "Export failed",
    "清除失败": "Clear failed"
  };

  const RUNTIME_MESSAGES = {
  "session.activity.idle": {
    "en": "Waiting for a new task",
    "zh-Hans": "等待新任务"
  },
  "session.activity.ended": {
    "en": "Session ended",
    "zh-Hans": "会话已结束"
  },
  "session.activity.thinking": {
    "en": "Thinking",
    "zh-Hans": "正在思考"
  },
  "session.activity.tool_running": {
    "en": "Running {tool}",
    "zh-Hans": "正在运行 {tool}"
  },
  "session.activity.awaiting_approval": {
    "en": "Waiting for your approval",
    "zh-Hans": "等待你批准"
  },
  "session.activity.awaiting_answer": {
    "en": "Waiting for your answer",
    "zh-Hans": "等待你回答"
  },
  "session.activity.compacting": {
    "en": "Compacting context",
    "zh-Hans": "正在压缩记忆"
  },
  "session.activity.completed": {
    "en": "Turn completed",
    "zh-Hans": "本轮已完成"
  },
  "session.activity.interrupted": {
    "en": "Turn interrupted",
    "zh-Hans": "本轮已中断"
  },
  "session.activity.failed": {
    "en": "Run failed",
    "zh-Hans": "运行失败"
  },
  "session.activity.plan_progress": {
    "en": "Plan progress {done}/{total}",
    "zh-Hans": "计划进度 {done}/{total}"
  },
  "session.activity.subagents_running": {
    "en": "{count} subagents running",
    "zh-Hans": "{count} 个子 Agent 正在运行"
  },
  "session.activity.background_tasks_running": {
    "en": "{count} background tasks still running",
    "zh-Hans": "{count} 个后台任务仍在运行"
  },
  "session.activity.permission_denied": {
    "en": "The operation was denied in the Agent",
    "zh-Hans": "操作已在 Agent 中拒绝"
  },
  "session.activity.waiting_for_provider_event": {
    "en": "Waiting for the Agent's next event",
    "zh-Hans": "等待 Agent 后续事件"
  },
  "session.activity.unknown_event": {
    "en": "Unrecognized event; the Provider version may be incompatible",
    "zh-Hans": "事件不识别，Provider 版本可能不兼容"
  },
  "attention.approval.title": {
    "en": "Approval required",
    "zh-Hans": "等待批准"
  },
  "attention.native_approval.title": {
    "en": "Approve in {provider}",
    "zh-Hans": "请在 {provider} 中批准"
  },
  "attention.question.title": {
    "en": "{provider} is asking a question",
    "zh-Hans": "{provider} 正在询问"
  },
  "attention.error.title": {
    "en": "Agent run failed",
    "zh-Hans": "Agent 运行失败"
  },
  "attention.interrupted.title": {
    "en": "Agent turn interrupted",
    "zh-Hans": "Agent 本轮已中断"
  },
  "attention.completion.title": {
    "en": "Task completed; waiting for confirmation",
    "zh-Hans": "任务已完成，等待确认"
  },
  "attention.approval.detail": {
    "en": "Review the operation and its impact in the original conversation.",
    "zh-Hans": "请在原对话中核对操作内容和影响。"
  },
  "attention.native_approval.detail": {
    "en": "Review and handle this request in {provider}.",
    "zh-Hans": "请在 {provider} 中查看并处理此请求。"
  },
  "attention.question.detail": {
    "en": "Answer directly in ActRealm. Answers are not written to local history.",
    "zh-Hans": "可直接在 ActRealm 回答；答案不会写入本地历史。"
  },
  "attention.risk.high_impact": {
    "en": "High-impact operation detected",
    "zh-Hans": "已识别到高影响操作"
  },
  "attention.risk.irreversible": {
    "en": "The operation cannot be undone after it is submitted",
    "zh-Hans": "提交后动作本身不可撤销"
  },
  "attention.risk.compound_syntax": {
    "en": "The command contains compound syntax",
    "zh-Hans": "命令包含组合语法"
  },
  "attention.risk.read_only_intent": {
    "en": "Read-only intent; this rule is not a security guarantee",
    "zh-Hans": "只读意图；规则提示不构成安全保证"
  },
  "attention.risk.undo_window": {
    "en": "The approval decision can be undone for 3 seconds",
    "zh-Hans": "批准决定可在 3 秒内撤回"
  },
  "attention.risk.side_effects": {
    "en": "May run project code or produce side effects",
    "zh-Hans": "可能执行项目代码或产生副作用"
  },
  "attention.risk.unknown_impact": {
    "en": "The impact of this operation is unknown",
    "zh-Hans": "此操作的影响未知"
  },
  "attention.risk.review_original": {
    "en": "Review the original window",
    "zh-Hans": "建议查看原窗口"
  },
  "interaction.claude_question.title": {
    "en": "Claude is asking",
    "zh-Hans": "Claude 正在询问"
  },
  "interaction.claude_elicitation.title": {
    "en": "Claude needs more information",
    "zh-Hans": "Claude 需要补充信息"
  },
  "quota.reason.agent_refresh_failed": {
    "en": "Agent quota refresh failed; showing the last captured value.",
    "zh-Hans": "Agent 额度刷新失败，显示上次记录。"
  },
  "quota.reason.agent_unavailable": {
    "en": "The Agent did not return account quota information.",
    "zh-Hans": "Agent 暂未提供账户额度信息。"
  },
  "interaction.agent_question.title": {
    "en": "Agent is asking",
    "zh-Hans": "Agent 正在询问"
  },
  "interaction.codex_user_input.title": {
    "en": "Codex is asking",
    "zh-Hans": "Codex 正在询问"
  },
  "jump.exact_conversation": {
    "en": "Open exact conversation",
    "zh-Hans": "精确打开对话"
  },
  "jump.terminal": {
    "en": "Open terminal",
    "zh-Hans": "打开对应终端"
  },
  "jump.app_only": {
    "en": "Open application",
    "zh-Hans": "只能打开应用"
  },
  "jump.unsupported": {
    "en": "Jump is not supported",
    "zh-Hans": "当前环境不支持跳转"
  },
  "quota.window.months": {
    "en": "{count} months",
    "zh-Hans": "{count} 个月"
  },
  "quota.window.claude_weekly": {
    "en": "Weekly · all models",
    "zh-Hans": "总周额度"
  },
  "quota.window.scoped_weeks": {
    "en": "{name} · {count} weeks",
    "zh-Hans": "{name} · {count} 周"
  },
  "quota.window.scoped": {
    "en": "{name} quota",
    "zh-Hans": "{name} 额度"
  },
  "quota.reason.cache_stale": {
    "en": "This is a historical quota value; waiting for a fresh Provider update.",
    "zh-Hans": "这是历史额度，正在等待 Provider 更新；不能作为当前剩余额度。"
  },
  "quota.window.weeks": {
    "en": "{count} weeks",
    "zh-Hans": "{count} 周"
  },
  "quota.window.days": {
    "en": "{count} days",
    "zh-Hans": "{count} 天"
  },
  "quota.window.hours": {
    "en": "{count} hours",
    "zh-Hans": "{count} 小时"
  },
  "quota.window.minutes": {
    "en": "{count} minutes",
    "zh-Hans": "{count} 分钟"
  },
  "quota.window.current_week": {
    "en": "This week",
    "zh-Hans": "本周"
  },
  "quota.window.extra_usage": {
    "en": "Extra usage",
    "zh-Hans": "额外用量"
  },
  "quota.reason.cache_missing": {
    "en": "Quota cache is missing. Enable the Claude quota bridge and complete one conversation.",
    "zh-Hans": "额度缓存不存在，请开启 Claude 额度桥并完成一次对话。"
  },
  "quota.reason.cache_unreadable": {
    "en": "Quota cache could not be read: {error}",
    "zh-Hans": "额度缓存不可读：{error}"
  },
  "quota.reason.cache_incompatible": {
    "en": "Quota cache schema is incompatible.",
    "zh-Hans": "额度缓存版本不兼容。"
  },
  "quota.reason.cache_invalid": {
    "en": "Quota cache could not be parsed.",
    "zh-Hans": "额度缓存解析失败。"
  },
  "quota.reason.cache_from_future": {
    "en": "Quota cache timestamp is later than the local clock.",
    "zh-Hans": "额度缓存时间晚于本机时间。"
  },
  "quota.reason.no_valid_window": {
    "en": "No verifiable quota window was found.",
    "zh-Hans": "没有找到可验证的额度窗口。"
  },
  "quota.reason.codex_rollout_missing": {
    "en": "No Codex rollout file was found.",
    "zh-Hans": "未找到 Codex rollout 文件。"
  },
  "quota.reason.codex_window_missing": {
    "en": "No verifiable quota window was found in the Codex rollout.",
    "zh-Hans": "Codex rollout 中没有可验证的额度窗口。"
  },
  "quota.reason.claude_refresh_failed": {
    "en": "Claude quota refresh failed. The last successfully captured values are shown.",
    "zh-Hans": "Claude 额度刷新失败，当前显示的是上次成功获取的数据。"
  },
  "quota.reason.codex_refresh_failed": {
    "en": "Codex quota refresh failed. The last successfully captured values are shown.",
    "zh-Hans": "Codex 额度刷新失败，当前显示的是上次成功获取的数据。"
  }
};
  const API_ERRORS = {
  "ANSWER_FAILED": {
    "en": "The answer could not be sent to the Agent",
    "zh-Hans": "回答未能发送给 Agent"
  },
  "AUTH_PERSIST_FAILED": {
    "en": "Companion authorization could not be saved",
    "zh-Hans": "伴生应用授权无法保存"
  },
  "COMPANION_NOT_FOUND": {
    "en": "The Companion connection was not found",
    "zh-Hans": "没有找到对应的伴生应用连接"
  },
  "COMPANION_SCOPE_REQUIRED": {
    "en": "This Companion does not have the required permission",
    "zh-Hans": "当前伴生应用没有这项操作权限"
  },
  "COMPANION_UNAUTHORIZED": {
    "en": "The Companion connection is invalid or revoked",
    "zh-Hans": "伴生应用连接无效或已撤销"
  },
  "CURRENT_TURN_REQUIRED": {
    "en": "Current-turn mode is required for backward timeline paging",
    "zh-Hans": "向前加载任务时间线时必须限定当前阶段"
  },
  "INVALID_CLIENT_NAME": {
    "en": "The Companion name is invalid",
    "zh-Hans": "伴生应用名称无效"
  },
  "INVALID_HISTORY_LIMIT": {
    "en": "The history task limit is invalid",
    "zh-Hans": "历史任务读取数量无效"
  },
  "INVALID_COMPANION_ID": {
    "en": "The Companion identifier is invalid",
    "zh-Hans": "伴生应用标识无效"
  },
  "INVALID_PAIRING_CODE": {
    "en": "The pairing code is invalid or already used",
    "zh-Hans": "配对码无效或已经使用"
  },
  "INVALID_SESSION_ID": {
    "en": "The task identifier is invalid",
    "zh-Hans": "任务标识无效"
  },
  "INVALID_TIMELINE_CURSOR": {
    "en": "Use only one timeline cursor",
    "zh-Hans": "任务时间线只能使用一个游标"
  },
  "PAIRING_EXPIRED": {
    "en": "The pairing code has expired",
    "zh-Hans": "配对码已经过期"
  },
  "PAIRING_UNAVAILABLE": {
    "en": "No Companion pairing request is available",
    "zh-Hans": "当前没有可用的伴生应用配对请求"
  },
  "AUTH_UNAVAILABLE": {
    "en": "Runtime authentication is unavailable",
    "zh-Hans": "Runtime 身份验证暂不可用"
  },
  "BACKUP_CLEAR_FAILED": {
    "en": "Configuration backups could not be cleared safely",
    "zh-Hans": "配置备份未能安全清除"
  },
  "BACKUP_DELETE_CONFIRMATION_REQUIRED": {
    "en": "Enter DELETE BACKUPS to clear configuration backups",
    "zh-Hans": "需要输入 DELETE BACKUPS 才能清除配置备份"
  },
  "CLAUDE_BRIDGE_CHANGE_FAILED": {
    "en": "The Claude quota bridge could not be updated",
    "zh-Hans": "Claude 额度桥更新失败"
  },
  "CLAUDE_OAUTH_DISABLED": {
    "en": "The official Claude OAuth quota endpoint is disabled",
    "zh-Hans": "Claude 官方 OAuth 额度接口未启用"
  },
  "CLAUDE_SIGN_IN_REQUIRED": {"en": "Sign in to Claude Code once, then refresh. No conversation is needed.", "zh-Hans": "请先登录 Claude Code，再刷新额度；无需发送对话。"},
  "CLAUDE_AUTH_REFRESH_FAILED": {"en": "Automatic Claude credential renewal failed. Check Claude Code sign-in or try again.", "zh-Hans": "Claude 凭据自动续期失败，请检查登录状态或重试。"},
  "CLAUDE_QUOTA_RATE_LIMITED": {"en": "Claude is limiting quota requests. Automatic refresh will retry later.", "zh-Hans": "Claude 额度请求被限流，将稍后自动重试。"},
  "CLAUDE_QUOTA_REFRESH_FAILED": {
    "en": "Claude quota refresh failed",
    "zh-Hans": "Claude 额度刷新失败"
  },
  "CLEAR_FAILED": {
    "en": "Local data could not be cleared",
    "zh-Hans": "本地数据清除失败"
  },
  "CODEX_REINSTALL_FAILED": {
    "en": "The Codex Hook could not be reinstalled",
    "zh-Hans": "Codex Hook 重新安装失败"
  },
  "COMMAND_MISMATCH": {
    "en": "The command does not match the current request",
    "zh-Hans": "命令与当前请求不匹配"
  },
  "COMMIT_TOO_EARLY": {
    "en": "The decision is still inside the undo window",
    "zh-Hans": "决定仍在撤回窗口内"
  },
  "CONNECTOR_ATTACH_FAILED": {
    "en": "The Connector could not be attached",
    "zh-Hans": "Connector 连接失败"
  },
  "DELETE_CONFIRMATION_REQUIRED": {
    "en": "Enter DELETE to clear local data",
    "zh-Hans": "需要输入 DELETE 才能清除数据"
  },
  "EXPORT_FAILED": {
    "en": "Local data could not be exported",
    "zh-Hans": "本地数据导出失败"
  },
  "INVALID_ACTION": {
    "en": "This action is invalid",
    "zh-Hans": "当前操作无效"
  },
  "INVALID_ANSWER": {
    "en": "The answer is invalid",
    "zh-Hans": "回答内容无效"
  },
  "INVALID_BOOTSTRAP": {
    "en": "The launch credential is invalid or expired",
    "zh-Hans": "启动凭据无效或已过期"
  },
  "INVALID_COMMAND_ID": {
    "en": "The command identifier is invalid",
    "zh-Hans": "命令标识无效"
  },
  "INVALID_HOST": {
    "en": "The address is not a trusted local host",
    "zh-Hans": "访问地址不是受信任的本机地址"
  },
  "INVALID_ORIGIN": {
    "en": "The request origin is not trusted",
    "zh-Hans": "请求来源不受信任"
  },
  "INVALID_RESTART_TOKEN": {
    "en": "The Runtime restart credential is invalid",
    "zh-Hans": "Runtime 重启凭据无效"
  },
  "INVALID_SETTINGS": {
    "en": "The settings are invalid",
    "zh-Hans": "设置内容无效"
  },
  "CHECKPOINT_NOT_FOUND": {
    "en": "The checkpoint was not found",
    "zh-Hans": "没有找到对应 Checkpoint"
  },
  "CHECKPOINT_INVALID": {
    "en": "The checkpoint request is invalid",
    "zh-Hans": "Checkpoint 请求无效"
  },
  "CHECKPOINT_GIT_FAILED": {
    "en": "The Git checkpoint could not be created or applied safely",
    "zh-Hans": "无法安全创建或应用 Git Checkpoint"
  },
  "CHECKPOINT_PREFLIGHT_FAILED": {
    "en": "Checkpoint recovery preflight did not pass",
    "zh-Hans": "Checkpoint 恢复预检未通过"
  },
  "ARTIFACT_REVEAL_FAILED": {
    "en": "Finder could not reveal the file. Please try again",
    "zh-Hans": "访达未能定位该文件，请重试"
  },
  "ARTIFACT_UNAVAILABLE": {
    "en": "The referenced file is unavailable or no longer belongs to this result",
    "zh-Hans": "关联文件已不可用，或已不属于当前结果"
  },
  "JUMP_FAILED": {
    "en": "The original window was not found, or app-control permission is missing",
    "zh-Hans": "没有找到原窗口，或 macOS 尚未授予应用控制权限"
  },
  "JUMP_UNSUPPORTED": {
    "en": "Jumping is not supported in the current environment",
    "zh-Hans": "当前环境不支持跳转"
  },
  "MANAGED_CONNECTOR_UNSUPPORTED": {
    "en": "This session does not support a managed Connector",
    "zh-Hans": "当前会话不支持托管 Connector"
  },
  "METRIC_RECORD_FAILED": {
    "en": "The local metric could not be recorded",
    "zh-Hans": "本地统计记录失败"
  },
  "MISSING_REQUEST_ID": {
    "en": "This request has no replyable request identifier",
    "zh-Hans": "当前请求没有可回复的请求标识"
  },
  "PROVIDER_CLIENT_MISSING": {
    "en": "The corresponding Agent client was not found",
    "zh-Hans": "没有找到对应的 Agent 客户端"
  },
  "PROVIDER_INSTALL_REQUIRED": {
    "en": "Install the corresponding Agent client first",
    "zh-Hans": "请先安装对应的 Agent 客户端"
  },
  "QUESTION_EXPIRED": {
    "en": "This question has expired and cannot be submitted",
    "zh-Hans": "这个问题已经过期，不能再提交"
  },
  "QUOTA_PERSIST_FAILED": {
    "en": "The quota result could not be saved locally",
    "zh-Hans": "额度结果无法保存到本机"
  },
  "QUOTA_REFRESH_FAILED": {
    "en": "Quota refresh failed",
    "zh-Hans": "额度刷新失败"
  },
  "QUOTA_REFRESH_IN_PROGRESS": {
    "en": "Quota refresh is already in progress",
    "zh-Hans": "额度正在刷新，请稍后再试"
  },
  "QUOTA_STATE_UNAVAILABLE": {
    "en": "Quota state is unavailable",
    "zh-Hans": "额度状态暂不可用"
  },
  "REQUEST_MISMATCH": {
    "en": "The request does not match the current attention item",
    "zh-Hans": "请求与当前待处理事项不匹配"
  },
  "RETENTION_FAILED": {
    "en": "The local retention policy could not be applied",
    "zh-Hans": "本地保留策略执行失败"
  },
  "RUNTIME_RESTART_FAILED": {
    "en": "Runtime restart failed",
    "zh-Hans": "Runtime 重启失败"
  },
  "RUNTIME_RESTART_TIMED_OUT": {
    "en": "Runtime restart timed out",
    "zh-Hans": "Runtime 重启超时"
  },
  "RUNTIME_RESTART_UNAVAILABLE": {
    "en": "This Runtime cannot restart automatically",
    "zh-Hans": "当前 Runtime 无法自动重启"
  },
  "SESSION_NOT_FOUND": {
    "en": "The corresponding task was not found",
    "zh-Hans": "没有找到对应任务"
  },
  "TASK_STILL_ACTIVE": {
    "en": "This task is still running or awaiting attention and cannot be archived or deleted",
    "zh-Hans": "任务仍在运行或等待处理，不能归档或删除历史"
  },
  "SETTINGS_READ_FAILED": {
    "en": "Local settings could not be read",
    "zh-Hans": "本机设置读取失败"
  },
  "SETUP_CHANGE_FAILED": {
    "en": "Agent setup could not be updated",
    "zh-Hans": "Agent 接入更新失败"
  },
  "SETUP_INSPECTION_FAILED": {
    "en": "Agent setup status could not be inspected",
    "zh-Hans": "Agent 接入状态检查失败"
  },
  "STALE_APPROVAL": {
    "en": "This approval request has expired",
    "zh-Hans": "这项批准请求已经过期"
  },
  "STALE_ATTENTION": {
    "en": "This attention item has changed or expired",
    "zh-Hans": "这项待处理事项已经更新或过期"
  },
  "STORAGE_ERROR": {
    "en": "Local storage is unavailable",
    "zh-Hans": "本地存储暂不可用"
  },
  "UNAUTHORIZED": {
    "en": "The local session has expired; reconnect to continue",
    "zh-Hans": "本机会话已失效，请重新连接"
  },
  "UNAUTHORIZED_MUTATION": {
    "en": "This request cannot modify local state",
    "zh-Hans": "当前请求没有修改权限"
  },
  "UNAUTHORIZED_WEBSOCKET": {
    "en": "Realtime connection authentication failed",
    "zh-Hans": "实时连接身份验证失败"
  },
  "UNKNOWN_ACTION": {
    "en": "Unknown action",
    "zh-Hans": "未知操作"
  },
  "UNKNOWN_BRIDGE_ACTION": {
    "en": "Unknown quota-bridge action",
    "zh-Hans": "未知额度桥操作"
  },
  "UNKNOWN_MANAGE_ACTION": {
    "en": "Unknown managed-session action",
    "zh-Hans": "未知托管操作"
  },
  "UNKNOWN_PROVIDER": {
    "en": "Unknown Agent type",
    "zh-Hans": "未知 Agent 类型"
  },
  "UNKNOWN_SETUP_ACTION": {
    "en": "Unknown setup action",
    "zh-Hans": "未知接入操作"
  },
  "UNSAFE_DATA_PATH": {
    "en": "The local data path failed its safety check",
    "zh-Hans": "本地数据目录未通过安全检查"
  },
  "RUNTIME_NOT_CONNECTED": {
    "en": "Runtime is not connected",
    "zh-Hans": "Runtime 尚未连接"
  },
  "RUNTIME_AUTH_FAILED": {
    "en": "Runtime authentication failed",
    "zh-Hans": "Runtime 身份验证失败"
  },
  "RUNTIME_SESSION_MISSING": {
    "en": "Runtime did not return a local session",
    "zh-Hans": "Runtime 没有返回本机会话"
  }
};

  function preference() {
    return global.localStorage?.getItem(STORAGE_KEY) || "system";
  }

  function resolvedLocale(selected = preference()) {
    if (selected === "en" || selected === "zh-Hans") return selected;
    const preferred = String(global.navigator?.language || "en").toLowerCase();
    return preferred.startsWith("zh") ? "zh-Hans" : "en";
  }

  function format(template, args = {}) {
    return String(template).replace(/\{([a-zA-Z0-9_]+)\}/g, (_, key) =>
      Object.prototype.hasOwnProperty.call(args, key) ? String(args[key]) : `{${key}}`
    );
  }

  function plural(value, one, many = `${one}s`) {
    return `${value} ${Number(value) === 1 ? one : many}`;
  }

  function translatePattern(text) {
    let match;
    if ((match = text.match(/^(\d+) 个任务$/))) return plural(match[1], "task");
    if ((match = text.match(/^(\d+) 个 Agent 已接入$/))) return `${match[1]} Agents connected`;
    if ((match = text.match(/^(\d+) 个已接入 · (\d+) 待处理$/))) return `${match[1]} connected · ${match[2]} pending`;
    if ((match = text.match(/^(\d+) 项接入待处理$/))) return `${match[1]} setup items pending`;
    if ((match = text.match(/^已接入 (\d+)$/))) return `${match[1]} connected`;
    if ((match = text.match(/^待处理 (\d+)$/))) return `${match[1]} pending`;
    if ((match = text.match(/^最久等待 (\d+) 分钟$/))) return `Longest wait: ${plural(match[1], "minute")}`;
    if ((match = text.match(/^(\d+) 个任务 · (\d+) 等待 · (\d+) 运行中 · (\d+) 已完成$/))) {
      return `${match[1]} tasks · ${match[2]} waiting · ${match[3]} running · ${match[4]} complete`;
    }
    if ((match = text.match(/^累计 (.+) Token$/))) return `Total ${match[1]} tokens`;
    if ((match = text.match(/^上下文 (\d+)%$/))) return `Context ${match[1]}%`;
    if ((match = text.match(/^估算 API 价格 (.+)$/))) return `Estimated API price ${match[1]}`;
    if ((match = text.match(/^计划 (\d+)\/(\d+)$/))) return `Plan ${match[1]}/${match[2]}`;
    if ((match = text.match(/^(\d+) 个子 Agent 正在运行$/))) return `${plural(match[1], "sub-agent")} running`;
    if ((match = text.match(/^剩余 (\d+)%$/))) return `${match[1]}% remaining`;
    if ((match = text.match(/^(\d+) 分钟前更新$/))) return `Updated ${plural(match[1], "minute")} ago`;
    if ((match = text.match(/^(\d+) 分钟$/))) return plural(match[1], "minute");
    if ((match = text.match(/^(\d+) 小时$/))) return plural(match[1], "hour");
    if ((match = text.match(/^(\d+) 天$/))) return plural(match[1], "day");
    if ((match = text.match(/^(\d+) 周$/))) return plural(match[1], "week");
    if ((match = text.match(/^(\d+) 个月$/))) return plural(match[1], "month");
    if ((match = text.match(/^(\d+) 秒$/))) return plural(match[1], "second");
    if ((match = text.match(/^已等 (.+)$/))) return `Waiting ${match[1]}`;
    if ((match = text.match(/^截止 (.+)$/))) return `Due ${match[1]}`;
    if ((match = text.match(/^队列 · 还有 (\d+) 项$/))) return `Queue · ${match[1]} more`;
    if ((match = text.match(/^风险标记：(.+)$/))) return `Risk: ${match[1]}`;
    if ((match = text.match(/^(\d+)\/(\d+)（进行中）$/))) return `${match[1]}/${match[2]} (in progress)`;
    if ((match = text.match(/^(\d+) 活跃 · (\d+) 待处理$/))) return `${match[1]} active · ${match[2]} pending`;
    if ((match = text.match(/^已恢复 · (\d+) 次$/))) return `Recovered · ${plural(match[1], "restart")}`;
    if ((match = text.match(/^(\d+) 事件 · (.+)$/))) return `${plural(match[1], "event")} · ${tr(match[2], "en")}`;
    if ((match = text.match(/^(\d+) 个连接$/))) return plural(match[1], "connection");
    if ((match = text.match(/^当前有 (\d+) 个请求正在等待。重启会安全放行这些请求并重新连接 Agent。$/))) {
      return `${plural(match[1], "request")} waiting. Restarting safely returns them to the Agent and reconnects.`;
    }
    if ((match = text.match(/^任务已清除，并交还 (\d+) 项待处理事项$/))) return `Task cleared; ${plural(match[1], "Outbox item")} returned`;
    if ((match = text.match(/^任务已清除；(\d+) 项仍需在 Outbox 或 Agent 原界面处理$/))) return `Task cleared; ${plural(match[1], "item")} ${Number(match[1]) === 1 ? "still requires" : "still require"} action in Outbox or the Agent`;
    if ((match = text.match(/^最近同步 · (.+)$/))) return `Last sync · ${match[1]}`;
    if ((match = text.match(/^(.+) 接入已移除$/))) return `${match[1]} setup removed`;
    if ((match = text.match(/^(.+) 配置已安全写入$/))) return `${match[1]} configuration saved safely`;
    if ((match = text.match(/^连接 (.+)$/))) return `Connect ${match[1]}`;
    if ((match = text.match(/^(.+) 图标$/))) return `${match[1]} icon`;
    if ((match = text.match(/^(.+) 客户端$/))) return `${match[1]} client`;
    if ((match = text.match(/^(.+) · 保留上次有效值$/))) return `${match[1]} · Showing last valid value`;
    if ((match = text.match(/^(.+) · 剩余 (\d+)%$/))) return `${match[1]} · ${match[2]}% remaining`;
    if ((match = text.match(/^(.+?)：(.+)$/))) {
      const prefix = tr(match[1], "en");
      if (prefix !== match[1]) return `${prefix}: ${match[2]}`;
    }
    return text;
  }

  function tr(text, locale = resolvedLocale()) {
    const source = String(text ?? "");
    if (locale !== "en") return source;
    return LITERALS_EN[source] || EXTRA_EN[source] || translatePattern(source);
  }

  function runtimeMessage(message, fallback = "", locale = resolvedLocale()) {
    if (!message?.code) return fallback;
    const entry = RUNTIME_MESSAGES[message.code];
    const template = entry?.[locale] || fallback || message.code;
    return format(template, message.args || {});
  }

  function apiError(code, locale = resolvedLocale()) {
    return API_ERRORS[String(code || "")]?.[locale];
  }

  function bootstrapPlan({ hashToken, csrfToken } = {}) {
    if (String(hashToken || "").trim()) return "bootstrap-first";
    if (String(csrfToken || "").trim()) return "snapshot";
    return "unauthenticated";
  }

  function translateStatic(root = global.document) {
    if (!root?.createTreeWalker) return;
    const document = root.nodeType === 9 ? root : root.ownerDocument;
    const walker = document.createTreeWalker(root, global.NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      if (!ORIGINAL_TEXT.has(node)) ORIGINAL_TEXT.set(node, node.nodeValue);
      const original = ORIGINAL_TEXT.get(node);
      const leading = original.match(/^\s*/)?.[0] || "";
      const trailing = original.match(/\s*$/)?.[0] || "";
      const core = original.trim();
      if (core) node.nodeValue = leading + tr(core) + trailing;
    }
    for (const element of root.querySelectorAll?.("[aria-label], [placeholder], [title]") || []) {
      let originals = ORIGINAL_ATTRIBUTES.get(element);
      if (!originals) {
        originals = {};
        for (const name of ["aria-label", "placeholder", "title"]) {
          if (element.hasAttribute(name)) originals[name] = element.getAttribute(name);
        }
        ORIGINAL_ATTRIBUTES.set(element, originals);
      }
      for (const [name, value] of Object.entries(originals)) element.setAttribute(name, tr(value));
    }
    if (global.document?.documentElement) {
      global.document.documentElement.lang = resolvedLocale() === "en" ? "en" : "zh-CN";
    }
  }

  function setPreference(selection) {
    const normalized = ["system", "zh-Hans", "en"].includes(selection) ? selection : "system";
    global.localStorage?.setItem(STORAGE_KEY, normalized);
    translateStatic(global.document);
    global.dispatchEvent?.(new global.CustomEvent("actrealm:language-changed", {
      detail: { selection: normalized, locale: resolvedLocale(normalized) },
    }));
  }

  const api = {
    STORAGE_KEY,
    LITERALS_EN,
    RUNTIME_MESSAGES,
    API_ERRORS,
    preference,
    resolvedLocale,
    format,
    plural,
    tr,
    runtimeMessage,
    apiError,
    bootstrapPlan,
    translateStatic,
    setPreference,
  };

  global.ActRealmI18n = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof window !== "undefined" ? window : globalThis);
