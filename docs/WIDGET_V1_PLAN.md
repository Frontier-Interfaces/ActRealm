# ActRealm Widget v1 开发计划书（施工版）

版本：v1.1（M14 实时用量修订版）· 2026-07-18
用途：本文档是 v1 的完整施工基线；已提交功能推进到 M14，并已完成本机安装与用户验收。当前事实状态见 `STATUS.md`，逐项验收见 `V1_ACCEPTANCE.md` 和各里程碑验证记录。
修订说明：原草案统一 60 秒批准时限经真实 Provider 探针和 Open Vibe Island / CodeIsland 生产经验复核后废止，改为 Claude 24 小时、Codex 1 小时；Runtime 缺失、连接失败、socket EOF 和主动 pass-through 仍立即交还原界面。M6-M13 的功能修订已补入 §19；M5 的 48 小时稳定性门禁仍未完成，因此不得宣称 v1 正式发布。其余参考审查决策见 `REFERENCE_REVIEW.md`。
上游文档：`AGENT_PRODUCT_PLAN.md`（产品总纲）、`SURVEY_INSIGHTS_UX_PLAN.md`（需求依据）、`COMPETITOR_ANALYSIS.md`（竞品与技术参考）。
交互基准：`bento-touch.html`（仅作为视觉 token 与手势参考；能力、模块和数据真实性冲突时以本文为准）。

---

## 目录

1. [产品定义与 v1 范围](#1-产品定义与-v1-范围)
2. [系统架构](#2-系统架构)
3. [技术选型（M0 后冻结）](#3-技术选型m0-后冻结)
4. [Provider 接入规格](#4-provider-接入规格)
5. [Hook 安装器规格](#5-hook-安装器规格)
6. [数据模型（SQLite DDL 级）](#6-数据模型sqlite-ddl-级)
7. [状态机规格](#7-状态机规格)
8. [命令风险提示器 v1](#8-命令风险提示器-v1)
9. [本地 API 规格](#9-本地-api-规格)
10. [UI 总体规格（1600×600）](#10-ui-总体规格1600600)
11. [模块一：待处理（Hero）](#11-模块一待处理hero)
12. [模块二：Agent 任务](#12-模块二agent-任务)
13. [模块三：额度](#13-模块三额度)
14. [Widget 网格引擎规格](#14-widget-网格引擎规格)
15. [通知与设置](#15-通知与设置)
16. [Onboarding 与异常态](#16-onboarding-与异常态)
17. [安全与隐私要求](#17-安全与隐私要求)
18. [工程结构与构建](#18-工程结构与构建)
19. [开发里程碑与验收标准](#19-开发里程碑与验收标准)
20. [测试要求](#20-测试要求)
21. [验证埋点（硬件立项依据）](#21-验证埋点硬件立项依据)
22. [风险与降级清单](#22-风险与降级清单)
23. [附录：Hook 配置与应答格式](#23-附录hook-配置与应答格式)

---

## 1. 产品定义与 v1 范围

### 1.1 一句话定义

> 一块常驻 1600×600 屏幕的本地 Agent 注意力与授权面板：实时显示每个 Agent 在干什么，把需要人决策的事项按优先级送到眼前；对官方 Hook 明确支持的单次权限请求，可直接批准/拒绝，故障或超时则自动交还原终端，数据不出本机。

### 1.2 战略角色

本产品是 **AI Display 硬件的需求验证载具**：软件形态先在用户的副屏/触摸屏上验证"注意力面"的价值，埋点数据（§21）决定硬件是否立项。因此所有架构决策必须满足：Runtime 与 UI 分离、UI 可直接迁移为硬件固件上的渲染层。

### 1.3 v1 范围（只做这些）

| 项 | 内容 |
|---|---|
| 模块 | **待处理（Hero）、Agent 任务、额度** —— 仅三个 |
| Provider | Claude Code CLI/Desktop 本机会话（P0）、Codex CLI/Desktop 本机会话（P0）、Gemini CLI（P1，仅轮级只读）；P0 的具体控制能力按每次事件的真实 reply channel 判定 |
| 接入模式 | **External Hook Control Mode**：Agent 仍由用户在原 CLI 或桌面客户端启动；Runtime 旁路观察事件，只在拥有官方 reply channel 时临时接管单次批准/问题链路。原生界面独占的批准只同步状态。存在受支持桌面 App 时不得强制安装全局 CLI |
| 直接控制 | 仅 `approve / deny / pass_through_to_terminal`；每个 Session 按实际 capability 显示，不按 Provider 名称硬编码 |
| 故障回退 | Runtime 不可用或 socket 断开时立即交还；正常等待达到 Provider 专属时限（Claude 24h / Codex 1h）时，Hook 无决定退出，Provider 恢复原生终端批准流程 |
| 本地动作 | 跳回终端（Runtime 执行 AppleScript/开终端）；完成确认（本地归档） |
| 平台 | macOS 优先（跳回终端仅 mac 实现）；Linux/Windows 可运行但跳回降级为提示 |
| 交付形态 | 单二进制 `actrealm`，`actrealm serve --open` 启动，浏览器全屏显示于 1600×600 屏 |

### 1.4 明确不做（v1 拒绝清单）

Coach、多 Agent 接力、决策记忆的语义匹配（仅做同类命令计数）、自动授权规则、灵动岛/三分栏视图、云端/账号/遥测、移动端、历史回放页、Gemini 批准应答、interrupt/steer/任意消息注入。v1.1 已把 Claude 官方问题 Hook、显式 Codex app-server Connector、Thread 恢复和 requestUserInput 纳入 M10-M12，并在 M13 加入 Provider 原生批准状态同步；这些能力不扩大 Hook-only 会话的所有权。Codex app-server 的 command/file/permissions 三类直接批准保留为拟议 M15，当前未实现。

### 1.5 能力边界（必须在 UI 中诚实呈现）

External Hook Control Mode 不等于 Managed Mode：它不拥有 Agent 会话本身。可直接回复的能力只来自当前事件的官方 reply channel：Hook `PermissionRequest`、Claude AskUserQuestion/Elicitation，或显式 managed Codex 的 `requestUserInput`。Provider 原生批准只观察 waiting/resolved。Session 与 Attention 投影必须携带 capability，而不是由前端按 Provider 猜测：

```json
{
  "observe": true,
  "approveViaHook": true,
  "denyViaHook": true,
  "passThroughToTerminal": true,
  "answerQuestion": false,
  "observeNativeApproval": true,
  "controlMode": "external_hook",
  "interrupt": false,
  "steer": false
}
```

---

## 2. 系统架构

### 2.1 进程模型（共 3 类进程）

```text
┌────────────────────────────────────────────────────────────────┐
│ 用户的 Mac                                                      │
│                                                                  │
│  Claude Code / Codex 本机 CLI 或 Desktop（用户启动，不归我们管）      │
│        │ 触发 Hook：把 JSON 写入子进程 stdin                       │
│        ▼                                                         │
│  [进程A] actrealm hook --provider claude|codex|gemini           │
│        │  · 生命周期 = 一次事件（毫秒级）；PermissionRequest 时      │
│        │    Claude 最多 24h / Codex 最多 1h；断线立即交还原终端       │
│        │  · Unix socket: ~/.actrealm/run/bridge.sock            │
│        ▼                                                         │
│  [进程B] actrealm serve  （常驻 Runtime，单实例）                │
│        │  · BridgeServer(socket) → 规范化 → SQLite → 投影          │
│        │  · HTTP+WS: 127.0.0.1:随机端口                           │
│        │  · 托管 Web 静态资源；执行跳回终端(osascript)              │
│        ▼                                                         │
│  [进程C] 浏览器（1600×600 屏，全屏/kiosk 模式）                    │
│           渲染三模块 Widget 网格                                   │
└────────────────────────────────────────────────────────────────┘
```

### 2.2 一条批准链路（本产品的命根子，全组件围绕它验收）

```text
Provider 触发 PermissionRequest
→ [A] 生成 requestId，200ms 内连接 [B]；连接失败则无输出退出
→ [A] 经 socket 发送事件并阻塞等待：Claude 最长 24h，Codex 最长 1h
→ [B] 事务写入：event + session状态=awaiting_approval + attention_item + 通知
→ [C] WS 收到 attention_changed → 渲染批准卡（含非权威风险提示）

分支 A · 面板批准/拒绝：
→ [C] POST /api/v1/commands {approve|deny, requestId, 幂等键}
→ [B] 进入 3 秒延迟提交（期间 [A] 仍挂起，未向 stdout 写决定，可撤回）
→ 3 秒后 [B] 经 socket 发 decision → [A] 写 Provider 对应的最小 JSON → 退出
→ command 状态仅记 decision_sent；收到后续 Provider 事件后才记 confirmed/resolved

分支 B · 交回原终端：
→ 用户点「去终端处理」→ [B] 向 [A] 发 pass_through
→ [A] 不向 stdout 写决定并退出 → Provider 显示原生批准提示
→ [B] 标记 attention=passed_through，并聚焦对应终端

分支 C · 故障/超时：
→ Runtime 崩溃、socket EOF、instanceId 改变或 Provider 专属时限到期
→ [A] 不重连、不写决定，立即退出 → Provider 原生流程接管
→ 旧 attention 标记 expired；绝不继续显示为可操作卡片

分支 D · Provider 原生批准（无 ActRealm reply channel）：
→ Hook `PreToolUse(request_permissions)` 或 managed Thread
  `waitingOnApproval` 明确表明原界面正在等待
→ [B] 创建无 requestId 的 observation-only native_approval，session 标记原界面等你
→ [C] 只显示「去 Agent 处理 / 待会提醒 / 忽略」，禁止显示允许/拒绝
→ 仅在匹配 `PostToolUse`、Thread 状态清除或其他可靠终态到达后中性消退
→ 不推断用户批准、拒绝或动作已经执行
```

**依据与约束**：官方 Hook 支持在 `PermissionRequest` 返回 allow/deny；无决定退出时继续 Provider 原生批准流程。Open Vibe Island / CodeIsland 证明了外部 UI + socket 中继的可行性。v1.1 已逐项审查两者的生产故障经验，但输出字段和兼容性仍以当前官方文档及 M0 针对当前 Provider 版本采集的 fixtures 为准，不照抄过期示例或实现代码。

### 2.3 Fail-open 铁律

任何时刻 Runtime 不可用（未启动/崩溃/socket 超时），hook 进程必须**静默退出且不向 stdout 写任何决定**——Agent 回到 Provider 原生流程。非 PermissionRequest 事件的 socket 发送超时 ≤ 200ms；发送失败时事件写入本地 spool 目录（`~/.actrealm/spool/`，上限 500 条/5MB，Runtime 启动时重放）。

PermissionRequest 的额外铁律：
- 连接 Runtime 的预算 ≤200ms；连接失败不落 spool（过期批准不可重放）；
- 等待中每 2 秒做轻量心跳；socket EOF / Runtime instanceId 变化立即无决定退出；
- Provider 专属 hard deadline（Claude 24h / Codex 1h）到期前向 Runtime 尽力发送 `passed_through(timeout)`，随后无决定退出；自动化测试必须可注入毫秒级短时限；
- Hook stdin 有独立 5 秒 hard deadline，Provider 管道不关闭时也必须静默 fail-open；
- Hook 不跨 Runtime 重连。Runtime 重启后，旧批准事项只能 expired，不能“恢复为 open”；
- Hook 退出前尚未写 stdout 时可撤回；一旦写出 allow/deny，UI 不得宣称仍可撤回。

### 2.4 单实例

启动时获取 `~/.actrealm/run/runtime.lock` 独占文件锁 → 监听成功后原子写 `runtime.json`（pid、port、instanceId、protocolVersion、startedAt、authToken 路径）→ 权限 0600。第二实例启动时读取 runtime.json、健康检查通过则直接打开浏览器指向现有实例后退出。

---

## 3. 技术选型（M0 后冻结）

以下选择是 v1 默认方案。M0 若证明 Provider 接入或进程模型不成立，允许通过 ADR 调整；M0 通过后冻结。本文选择原生 Web UI，明确覆盖上游总纲中的 React 建议，仅限这个三模块 v1，后续扩展需重新评估。

| 层 | 选择 | 理由/依据 |
|---|---|---|
| Runtime 语言 | **Rust** (edition 2021, stable) | 常驻低内存、单二进制分发、迁移硬件；上游计划书已定 |
| 异步运行时 | tokio | 生态标准 |
| HTTP/WS | axum + tokio-tungstenite（axum 内建 ws） | 上游计划书已定 |
| 数据库 | **rusqlite**（bundled 特性）+ 单 writer 线程 + WAL | 明确事务优先，不用 ORM/SQLx |
| 序列化 | serde / serde_json | — |
| CLI | clap（derive） | 子命令：serve/hook/install-hooks/uninstall-hooks/doctor/status/export |
| 日志 | tracing + tracing-subscriber（文件轮转） | 默认脱敏（§17） |
| 静态资源 | rust-embed（直接嵌入 web 文件） | 单文件交付 |
| 系统交互 | std::process 调 `osascript`（跳回终端）、`open`（开浏览器） | Open Island 同方案 |
| Web UI | **原生 HTML/CSS/JS，零框架、零构建**（延续原型） | v1 界面复杂度不需要 React；无 node 工具链，Claude Code 开发调试最快；文件 <100KB |
| Web↔Runtime | fetch + WebSocket + 单一全局 store（手写 ~50 行观察者） | — |
| 进程通信 | Unix Domain Socket（mac/linux）；Windows 用 127.0.0.1 环回 TCP + token（P2） | Open Island / CodeIsland 验证 |

**明确禁止**：Electron、React/Vue、SQLx/ORM、任何云 SDK、任何遥测库。

---

## 4. Provider 接入规格

以下事件清单以官方文档与当前本机 Provider 版本为准；第三方实现仅作参考。M0 必须采集真实脱敏 fixtures，并记录 `provider/version/event/schema/capability`，后续实现不得依赖未经 fixture 验证的字段。

### 4.1 Claude Code（Hook 覆盖最全，P0）

配置文件：`~/.claude/settings.json`。v1 订阅事件：

| 事件 | 用途 | 需要应答 |
|---|---|---|
| `SessionStart` | 建会话（source: startup/resume/clear/compact） | 否 |
| `SessionEnd` | 结束会话 | 否 |
| `UserPromptSubmit` | 进入 thinking 态；记录 prompt 首行作任务标题（可关） | 否 |
| `PreToolUse` | 进入 tool 态；`tool_name`+`tool_input` 生成活动文案与非权威风险提示 | 可选（v1 不用它放宽 Provider 权限） |
| `PostToolUse` | 回到 thinking 态 | 否 |
| `PostToolUseFailure` | 记 error 计数（不单独成卡，v1 仅在 StopFailure 成卡） | 否 |
| `PermissionRequest` | **批准卡**。最长挂起 24 小时；支持 allow/deny/pass-through | **是** |
| `Notification` | 更新活动文案（不做关键词解析——CodePulse 教训 #6） | 否 |
| `Stop` | 轮结束 → completion 卡（若该轮有文件改动）或回 idle | 否 |
| `StopFailure` | error 卡 | 否 |
| `SubagentStart/Stop` | 活动态"派了 N 个子 Agent" | 否 |
| `TaskCreated/TaskCompleted` | 计划进度 n/m（Agent 任务模块进度条唯一合法来源） | 否 |
| `PreCompact` | 活动态"压缩记忆" | 否 |

字段必须按事件解析，**不得假设全事件通用**。公共候选字段包括 `session_id, prompt_id, cwd, hook_event_name, transcript_path, permission_mode`；事件特有字段包括 `tool_name, tool_input, tool_use_id, prompt, last_assistant_message, error` 等。Claude `PermissionRequest` 没有 `tool_use_id`，批准关联一律使用 ActRealm 自己生成的 `requestId`。`transcript_path` 一律不读取。

### 4.2 Codex CLI / Desktop 本机会话（P0）

配置优先写入独立的 `~/.codex/hooks.json`，避免破坏用户 `config.toml` 的格式与未知字段；若用户已有同层 inline `[hooks]`，安装器必须识别并避免重复。当前版本 Hooks 默认启用；`[features].codex_hooks` 只作为旧版兼容别名，不写入新配置。v1 订阅 `SessionStart, UserPromptSubmit, PermissionRequest, Stop`（默认低噪音集），`PreToolUse/PostToolUse` 作为可选开关。

注意事项（全部来自实测记录）：
- Hook 安装后需要用户在 Codex 官方交互界面内 `/hooks` 手动信任——**Onboarding 必须引导，不可绕过**；桌面用户没有全局 CLI 时，使用 App 内置 Codex 可执行文件进入该审查流程；
- `PreToolUse/PostToolUse` 不是完整审计边界，不得宣称覆盖所有 shell、文件编辑与 WebSearch 路径；
- `PermissionRequest` 由 ActRealm 主动限制为 1 小时；其余事件 hook 进程自身 hard timeout 250ms（fail-open）；
- Codex `PermissionRequest` 输出使用官方最小 JSON，不附加当前不支持的 `continue/suppressOutput` 字段；
- 多个匹配 Hook 会并发运行，任一 deny 可压过 allow。因此 stdout 写出只代表 `decision_sent`，不能当成 Provider 已接受。

### 4.3 Gemini CLI（P1，只读粗粒度）

配置：`~/.gemini/settings.json`。仅订阅 `SessionStart, BeforeAgent, AfterAgent, SessionEnd, Notification`。fire-and-forget，无应答、无批准卡。UI 侧活动态只显示"干活中"（§12 诚实降级）。注意：Gemini hook 同步执行且 stdout 混入任何非 JSON 文本会破坏 Agent——hook 进程在 gemini 模式下**绝不写 stdout**。

### 4.4 统一事件信封（socket 上的内部协议）

hook 进程发给 Runtime 的每条消息：

```json
{
  "v": 1,
  "id": "uuidv7-event-envelope",
  "requestId": "uuidv7-stable-for-this-permission-request-or-null",
  "provider": "claude|codex|gemini",
  "providerSessionId": "...",
  "providerTurnId": "...|null",
  "promptId": "...|null",
  "role": "primary",
  "receivedAt": 1783920000123,
  "deadlineAt": 1783920060123,
  "raw": { "...provider 原始 payload 原样..." },
  "needsReply": true,
  "term": { "app": "iTerm", "sessionId": "...", "tty": "/dev/ttys003", "title": "..." }
}
```

- `role` 字段从环境变量 `ACTREALM_ROLE` 读取，预留 Coach 递归防护（v1 恒 primary）；
- 环境变量 `ACTREALM_SKIP_HOOKS=1` 时 hook 进程立即退出（对齐 Open Island 的 SKIP 机制）；
- 单条消息上限 256KB，超限截断 `tool_input`/`tool_response` 并打标记；
- `requestId` 由 hook 进程生成；同一 envelope 重放必须复用，不能重新生成；
- `needsReply=true` 用于 PermissionRequest，以及 Claude AskUserQuestion / Elicitation；Codex requestUserInput 只来自显式 app-server Connector，不伪装成 Hook 能力；
- Runtime → Hook 回复帧允许批准决定、Claude 问题答案、Elicitation 响应、pass-through 与 ping；事件类型和回复类型不匹配时 stdout 为空并交回 Provider；
- 活动 waiter 只存在于 Runtime 内存中，不把 socket 句柄伪装成可恢复数据库字段。

---

## 5. Hook 安装器规格

命令：`actrealm install-hooks [claude|codex|gemini|all]` 与 Onboarding UI 共用同一实现。

硬性要求（每条都是 CodePulse/社区踩坑的规避）：

1. 修改前把原配置备份到 `~/.actrealm/backups/<file>.<timestamp>`；
2. **语义合并**：Claude 解析 JSON；Codex 优先合并 `~/.codex/hooks.json`。仅追加/移除自己的 matcher group，不整份重写、不动用户既有 hook 和未知字段；
3. 我们写入的每个条目带识别标记（command 路径含 `actrealm`），卸载时只删带标记的；
4. 文件锁 + 写临时文件 + 原子 rename；
5. hook 命令指向稳定路径 `~/.actrealm/bin/actrealm`（安装时自拷贝，避免用户移动主程序导致 hook 失效——Open Island 同方案）；
6. 写入的 hook 配置必须显式设置超时：Claude PermissionRequest = 86400s、Codex PermissionRequest = 3600s；非 Permission 事件 timeout ≤5s；
7. 安装/升级后若 Hook 定义哈希变化，Codex 必须重新信任；Onboarding 不得显示“已接入”直到真实事件验证通过；
8. `actrealm doctor`：检测 CLI 与版本、hook 文件、Codex 信任状态、Runtime/socket、测试事件、控制回环与 pass-through，输出三色结果。
9. 安装意图持久化为 untouched / installed / uninstalled 三态；自动修复不得重装用户已主动卸载或手工移除的 Hook；
10. 支持 `CODEX_HOME`，识别 canonical `hooks` 与 legacy `codex_hooks`；配置无法解析时只报告、备份，不得猜测重写。

---

## 6. 数据模型（SQLite DDL 级）

库文件：`~/.actrealm/data.sqlite`（WAL，权限 0600）。所有写入经单 writer 线程，**一个业务动作一个事务**。

```sql
CREATE TABLE sessions (
  id TEXT PRIMARY KEY,              -- ActRealm 内部 UUIDv7
  provider TEXT NOT NULL,           -- claude|codex|gemini
  provider_session_id TEXT NOT NULL,
  cwd TEXT, project TEXT,           -- project = cwd 最后一段
  model TEXT, permission_mode TEXT,
  term_app TEXT, term_session_id TEXT, term_tty TEXT, term_title TEXT,
  exec_state TEXT NOT NULL DEFAULT 'idle',   -- §7 状态机
  approval_owner TEXT,              -- widget|terminal|null
  activity TEXT,                    -- 当前活动文案（Agent任务模块直读）
  activity_since INTEGER,
  plan_done INTEGER, plan_total INTEGER,     -- TaskCreated/Completed 推导，可空
  started_at INTEGER NOT NULL, last_event_at INTEGER NOT NULL,
  ended_at INTEGER,
  UNIQUE(provider, provider_session_id)
);
CREATE TABLE turns (
  id TEXT PRIMARY KEY,              -- ActRealm 内部 UUIDv7
  session_id TEXT NOT NULL,
  provider_turn_id TEXT, prompt_id TEXT, ordinal INTEGER NOT NULL,
  state TEXT NOT NULL DEFAULT 'running', -- running|response_finished|failed
  started_at INTEGER NOT NULL, ended_at INTEGER,
  UNIQUE(session_id, ordinal),
  FOREIGN KEY(session_id) REFERENCES sessions(id)
);
CREATE TABLE events (               -- append-only，原始 payload 默认不存
  id TEXT PRIMARY KEY, request_id TEXT,
  session_id TEXT NOT NULL, turn_id TEXT, provider TEXT NOT NULL,
  type TEXT NOT NULL,               -- 规范化类型 approval.requested 等
  tool_name TEXT, summary TEXT,     -- summary 为脱敏后单行摘要
  occurred_at INTEGER NOT NULL, ingest_seq INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(turn_id) REFERENCES turns(id)
);
CREATE TABLE attention_items (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL, provider TEXT NOT NULL, project TEXT,
  turn_id TEXT, request_id TEXT UNIQUE, -- request_id 仅 approval 必填
  kind TEXT NOT NULL,               -- approval|question|error|completion
  title TEXT NOT NULL, detail TEXT,
  command_preview TEXT,             -- 脱敏摘要；完整命令仅在活动 waiter 内存中保存
  risk TEXT NOT NULL,               -- low|med|high|unknown  (§8)
  risk_notes TEXT,                  -- JSON 数组：影响说明条目
  dedupe_key TEXT UNIQUE NOT NULL,  -- approval=requestId；其他=session+turn+kind
  state TEXT NOT NULL DEFAULT 'open',
  -- open|committing|decision_sent|passed_through|resolved|expired
  expires_at INTEGER,
  created_at INTEGER NOT NULL, resolved_at INTEGER, resolution TEXT,
  FOREIGN KEY(session_id) REFERENCES sessions(id),
  FOREIGN KEY(turn_id) REFERENCES turns(id)
);
CREATE TABLE commands (
  id TEXT PRIMARY KEY,              -- 客户端生成，幂等键
  attention_id TEXT NOT NULL, request_id TEXT,
  action TEXT NOT NULL,             -- approve|deny|pass_through|ack|snooze
  state TEXT NOT NULL,
  -- pending_commit|decision_sent|confirmed|passed_through|undone|failed
  created_at INTEGER NOT NULL, sent_at INTEGER, confirmed_at INTEGER,
  error_code TEXT,
  FOREIGN KEY(attention_id) REFERENCES attention_items(id)
);
CREATE TABLE approval_stats (       -- 仅展示历史批准/拒绝事实，不驱动自动授权
  project TEXT NOT NULL, risk_class TEXT NOT NULL, category TEXT NOT NULL, -- 如 test/build/install
  approve_count INTEGER DEFAULT 0, deny_count INTEGER DEFAULT 0,
  last_at INTEGER, PRIMARY KEY(project, category, risk_class)
);
CREATE TABLE quota_snapshots (
  provider TEXT NOT NULL, window TEXT NOT NULL,  -- 5h|7d|weekly
  used_pct REAL, resets_at INTEGER, source TEXT, -- source: statusline|rollout|unavailable
  captured_at INTEGER NOT NULL, PRIMARY KEY(provider, window)
);
CREATE TABLE metrics_daily (        -- §21 验证埋点
  day TEXT PRIMARY KEY,             -- YYYY-MM-DD
  approval_requests INTEGER DEFAULT 0,
  widget_approvals INTEGER DEFAULT 0, widget_denials INTEGER DEFAULT 0,
  pass_through_manual INTEGER DEFAULT 0, pass_through_timeout INTEGER DEFAULT 0,
  decision_response_ms_total INTEGER DEFAULT 0, decision_response_count INTEGER DEFAULT 0,
  banners_shown INTEGER DEFAULT 0,
  sessions_observed INTEGER DEFAULT 0, app_opened INTEGER DEFAULT 0
);
CREATE TABLE settings ( key TEXT PRIMARY KEY, value TEXT );
```

保留策略：`events` 90 天自动清理（可配置）；`attention_items`/`commands` 永久，但只保留脱敏摘要。完整命令、tool input、socket waiter 只存在于内存，批准结束或 pass-through 后立即释放。

启动一致性修复：Runtime 启动时，所有 `open|committing|decision_sent` 的 approval 若没有同一 `requestId` 的活动 waiter，一律标记 `expired`；不得恢复为可操作的 open 卡片。

---

## 7. 状态机规格

### 7.1 Session 执行态（`sessions.exec_state`）

```text
idle → thinking(UserPromptSubmit/BeforeAgent)
thinking → tool_running(PreToolUse) → thinking(PostToolUse)
thinking|tool_running → awaiting_approval(PermissionRequest, approval_owner=widget)
awaiting_approval → awaiting_approval(pass_through, approval_owner=terminal)
thinking|tool_running → awaiting_approval(native request_permissions / waitingOnApproval,
                                          approval_owner=terminal, no requestId)
awaiting_approval → thinking(决定发送后收到下一条运行事件)
任意 → response_finished(Stop/AfterAgent) → idle
任意 → failed(StopFailure)
SubagentStart 计数 +1 / SubagentStop -1（>0 时活动文案显示子 Agent）
PreCompact → compacting（短暂态，下一事件覆盖）
```

规则（全部必须有单元测试）：
- 同一 `event.id` 重复投递 → 幂等丢弃；
- 迟到事件不得把 `response_finished/failed` 拉回运行态；新 `UserPromptSubmit` 才开新轮；
- 未知 `hook_event_name` 不丢弃：记 events 表 type=`unknown`，Agent 任务模块的该 Provider 行显示"⚠ 事件不识别（可能版本不兼容）"；
- allow/deny 写入 stdout 只产生 `decision_sent`，不能直接把 Session 拉回 thinking；必须等待下一条 Provider 事件作为继续证据；
- pass-through 或原生批准后 Session 仍为 `awaiting_approval`，但 `approval_owner=terminal`，UI 显示“原界面等你”；
- native approval 没有 requestId，不允许 allow/deny；Notification/普通 tool running 不能覆盖明确 waiting，只有匹配的 Provider lifecycle/status transition 才中性清除；
- Runtime 启动时找不到活动 waiter 的 approval 一律 expired，禁止恢复成 open；
- 会话 30 分钟无事件且无挂起批准 → 从主列表过滤但保留本地历史；
- `SessionEnd` 缺失时结合捕获的父进程/App/TTY 存活、下一条 Hook 和超时规则，投影为观察中、等待新事件、失去控制、结束或空闲。

### 7.2 AttentionItem 生成规则

| 触发 | kind | 去重键 | 备注 |
|---|---|---|---|
| PermissionRequest | approval | requestId | 不依赖 Provider 的 tool_use_id；完整命令仅在 waiter 内存中 |
| Provider native approval | native_approval | session+turn+native-source | 无 requestId，只观察；与 live Hook approval 去重 |
| Claude AskUserQuestion / Elicitation | question | requestId | 官方 reply channel；secret 只在 DOM/请求/内存/一次回复中 |
| Managed Codex requestUserInput | question | server request id | 只有显式 attach 且 capability 成功时可回答 |
| StopFailure | error | session+turn+error | detail=error 首行（脱敏） |
| Stop 且本轮有 PostToolUse(写类工具) | completion | session+turn+completion | "干完了，等你确认" |
| Notification(claude, 显式 question 类型字段) | question_notice | session+notification_id | **仅当 payload 有结构化类型字段**；不做文案关键词猜测；无 reply channel 时只跳转 |

批准生命周期：

```text
open → committing → decision_sent → resolved
  ├→ open                    # 3 秒内 undo
  ├→ passed_through          # 用户主动交回终端
  └→ expired                 # deadline / socket EOF / Runtime 重启
```

`decision_sent` 后 30 秒仍无后续事件时，卡片显示“决定已发送，但尚未观察到 Agent 继续”，不得伪造 resolved。非 approval 事项仍支持 `snooze` 10 分钟后回 open。

Native approval 生命周期独立为 `open → provider_handled/expired`。`provider_handled`
只表示可靠 Provider 信号证明“不再等待”，不得映射成 approve、deny 或 executed。

---

## 8. 命令风险提示器 v1

纯规则表，同步执行，无 LLM。它只生成帮助用户阅读的**非权威提示**，不是安全边界，也不能替代 Provider 权限、沙箱或企业 deny 规则。输入 `tool_name + command 字符串`，输出 `risk + risk_notes[]`。

```text
匹配顺序：无法可靠解析或出现 shell 组合语法先判 unknown；再判 high；最后才允许命中 low
HIGH（永不淡化）：rm -rf / rm 带通配、git push（含 --force 加重）、sudo、
  curl|wget 管道执行、chmod 777、DROP TABLE、> 覆盖系统路径、
  网络外发（curl -d/POST 到非 localhost）、docker system prune、kill -9 1
LOW：仅限成功解析、无管道/重定向/替换/变量展开、目标路径已证明在工作区内的
  只读命令（git status|git diff|git log，以及受限参数的 ls/rg）
MED：测试、构建、lint、format（会执行仓库代码，可能写文件或联网）、
  包安装、git commit、文件写入、mkdir/mv/cp 工作区内
UNKNOWN：其余一切 → 显示“我不认识这个命令的影响，建议查看原窗口”
```

`risk_notes` 禁止生成“不会改文件”“一定安全”等不可证明结论。low → `["只读意图（规则提示，非安全保证）","↩ 3 秒内可撤回批准决定"]`；med → `["可能执行项目代码或产生副作用","建议核对原命令"]`；high → `["⚠ <已识别的具体影响>","提交后动作本身不可撤销"]`。历史批准次数只能作为事实旁注，不能单独把风险降级或生成自动批准建议。分类表放独立文件 `risk_rules.toml`，热更新后只影响新请求。

---

## 9. 本地 API 规格

绑定 `127.0.0.1` 随机端口。首次打开经一次性 bootstrap token 换 HttpOnly Cookie（§17）。

```text
GET  /api/v1/health            → { ok, version, protocolVersion }
GET  /api/v1/snapshot          → { sessions[], attention[], quota[], stats }   # 首屏一次拉全
POST /api/v1/commands          → { id, attentionId, requestId?, action }
                                  action=approve|deny|pass_through|ack|snooze
POST /api/v1/commands/:id/undo → 200 | 409(已提交)
POST /api/v1/jump              → { sessionId }   # Runtime 执行 osascript 聚焦终端
GET  /api/v1/settings / PUT 同路径
GET  /api/v1/export            → 打包 JSON 下载
WS   /api/v1/ws                # 服务端推送，见下
```

WS 推送帧（服务端 → 客户端，全量小对象，不做增量 diff）：

```json
{ "type": "attention_changed", "items": [ ...AttentionItem 投影... ] }
{ "type": "sessions_changed",  "sessions": [ ...含 exec_state/activity/plan... ] }
{ "type": "quota_changed",     "quota": [ ... ] }
{ "type": "command_state",     "id": "...", "state": "pending_commit|decision_sent|confirmed|passed_through|undone|failed" }
```

Command 提交语义：
- approve/deny：POST 后返回 `pending_commit`，启动 3 秒定时器；undo 恢复 open；到期且 waiter 仍活动才写应答并进入 `decision_sent`；
- pass_through：无延迟，立即通知 Hook 无决定退出，再聚焦终端；完成后进入 `passed_through`；
- ack/snooze：无延迟（本地动作可逆）；
- waiter 不存在、requestId 不匹配或已过 deadline → `409 STALE_APPROVAL`，前端立即移除操作按钮；
- 同一 id 重复 POST 返回当前状态；同一 requestId 只允许一个终态命令。approve/deny/pass-through 并发时，由数据库事务中第一个取得 open 状态者胜出；
- `decision_sent` 不是成功 ack；只有后续 Provider 事件能推进到 `confirmed/resolved`。

---

## 10. UI 总体规格（1600×600）

### 10.1 设计基线

- 视觉：参考 `bento-touch.html` 的浅灰底、白卡、圆角和色彩 token；正式实现另建三模块 v1 页面，禁止照抄原型中的 Coach、输入回答、假进度、假费用和五模块布局；
- 触屏：模块拖拽区域禁选择/长按；正文与诊断信息允许复制；所有可点目标 ≥44px，并兼容系统辅助缩放；
- 布局：12×6 虚拟网格铺满视口。**v1 默认布局（三模块）**：

```text
┌──────────────┬─────────────┬────────┐
│  待处理       │  Agent 任务  │  额度   │
│  x0 y0 w5 h6 │ x5 y0 w4 h6 │ x9 y0  │
│              │             │ w3 h6  │
└──────────────┴─────────────┴────────┘   5+4+3=12，全高，天然铺满
```

- v1 使用固定三栏布局，不做拖动/缩放/显隐；网格引擎保留为 v1.1（§14），避免抢占直接控制闭环的测试预算；
- 顶栏：logo、`Live · 本地` 状态点（WS 断开变灰 + "重连中"）、设置（通知规则入口）。

### 10.2 全局状态

| 状态 | 表现 |
|---|---|
| WS 断开 | 顶栏状态点变灰，全页蒙 2% 灰，横幅"与 Runtime 失去连接，正在重连"；自动指数退避重连，恢复后重新拉 snapshot |
| 无 Agent 接入 | 进 Onboarding（§16） |
| 空队列 | 待处理模块转"安静态"（§11.5） |

---

## 11. 模块一：待处理（Hero）

数据源：`attention[]`（WS 实时），按 `kind 权重(error 4 > approval 3 > question 2 > completion 1) → created_at 升序` 排序，一次只显示一件。

### 11.1 卡片结构（自上而下）

1. 徽章行：橙色呼吸徽章 `N 件等你 · <why>`（why: 阻塞类="任务停着"，completion="不着急"）
2. 标题：模板生成——approval=`想运行 <命令摘要>，等你点头`；error=`<摘要>，停下来了`；completion=`干完了「<任务名>」，等你确认`
3. Agent 行：Provider 图标 + 名称 + project
4. **提示块**（灰底圆角）：只呈现事实与非权威风险提示。历史只能写“过去 N 次同类请求：批准 X / 拒绝 Y”，不得仅凭历史生成“建议批准”；high/unknown 恒为“建议转到原终端核对”
5. **影响胶囊行**：直接渲染 risk_notes（绿 ✓ / 橙 ⏱ / 红 ⚠）
6. capability 按钮行：replyable approval 才显示 `批准 / 不行 / 去终端处理`；native approval 只显示 `去 Agent 处理 / 待会提醒 / 忽略`；**高风险 replyable approval** 主按钮变为 `去终端核对`，`批准…` 降为次按钮且点击弹二次确认，**触屏不提供高风险的滑动/回车捷径**
7. 分页行：圆点分页（可点）+ `第 n/N 件` + 滑动提示

### 11.2 交互

- 内容区左右滑切换事项（原型手势参数沿用：位移 >70px 触发、跟手 0.6 阻尼）；
- approve/deny 后：卡片暂时移出 open 队列 → 底部撤回条 `批准 · 3 秒后提交 [撤回]`；提交后显示“决定已发送”，直到后续事件确认；写回失败则恢复卡片；
- `去终端处理` 无撤回期：立即 pass-through，再聚焦原终端；卡片标记 passed_through，不再允许面板批准；
- native approval 没有 approve/deny/pass-through 命令；`忽略` 只隐藏本地展示，不向 Provider 声称处理结果，后续明确 resolution 仍更新 session；
- error 卡按钮：由事件内容生成不了选项时只给 `跳过去` + `标记已解决`；
- question 卡：Claude AskUserQuestion/Elicitation 和 managed Codex requestUserInput 按 schema 生成本地表单；没有官方 reply channel 的 question_notice 只有 `跳过去回答` + `待会`，绝不做任意消息注入；
- completion 卡：`没问题，收工`（ack，无延迟期）+ `跳过去看看`。

### 11.3 新事项到达

不抢当前卡：若用户停留在某卡 >2 秒，新事项只更新徽章计数与分页点，并短促闪一次徽章；仅当队列原本为空时直接展示新卡。

### 11.4 通知联动

按 §15 规则决定是否弹页内横幅（右上角，320px，带「查看」按钮）+ 可选提示音。

### 11.5 安静态（队列空）

绿色调："现在没有需要你处理的任务 ☕️" + 副行"预计下一次需要你 ≈ X 分钟（依据：在跑会话的历史平均轮时长；无数据不显示）" + 底行今日微统计"今天你已处理 N 件"。**禁止虚构数字：所有数值必须来自 metrics_daily / sessions 真实计算。**

---

## 12. 模块二：Agent 任务

数据源：`sessions[]`（WS 实时）。只展示仍在活动、仍有待处理事项，或最后一次
活动不超过 30 分钟的真实会话；无真实会话时显示模块空态，不为 Provider 构造
占位会话。结束且超过 30 分钟的会话只保留在本地历史，不占据主列表。

### 12.1 行结构

`[Provider 图标] [Provider 会话标题 + 任务内容 + 当前模型 + 活动行(动效) + 进度条(条件)] [右侧状态标签]`

- 可验证的 Provider 会话标题作为大标题；其下直接展示最近一次用户任务的限长、
  单行摘要，不添加“当前”或“当前任务”前缀；再下一行只显示当前模型。无
  Provider 标题时才用任务摘要回退。不得以用户名、项目名或 Provider 名称冒充。
  Claude 官方 `session_title`、本地
  custom/AI title 与 Codex 本地 thread name 必须按来源标记、限量读取，且只持久化
  规范化后的标题和来源；原始完整 prompt、transcript 内容与路径均不得进入浏览器
  快照；
- Claude/Codex 使用各自可辨识的本地图像标识，不用 `Cl` / `Co` 字母占位；
  第三方图标资产只允许来自许可兼容来源并保留 notices；

- 状态标签优先级：`等你`(橙, ActRealm 拥有 reply channel) > `原界面等你`(橙描边, Provider 原生批准) > `在跑`(蓝) > `空闲`(灰)；多件等待显示 `等你 ×N`；
- 点击行为：有等待 → Hero 切到该 Agent 的事项；待处理卡的“在 Agent 任务中
  查看”反向选择对应 session，置顶、高亮并滚动到可见位置；在跑 → toast 显示
  完整活动；空闲 → 无操作。

### 12.2 实时活动状态机（8 态，映射即事实来源）

| UI 态 | 事件来源 | 文案与动效 |
|---|---|---|
| 思考中 | UserPromptSubmit / PostToolUse 后 | `●●●波浪 正在思考… <n> 秒`（秒数=now-activity_since） |
| 执行工具 | PreToolUse | `▌闪烁 正在运行 <命令摘要>` / 写文件类 → `正在改 <文件名>`（摘要截 40 字符） |
| 等你 | PermissionRequest / question / native approval | ActRealm 接管时 `等你批准 · 剩余 <t>`；pass-through 后 `原界面等你处理`；native approval 只显示 `原界面请求批准 · ActRealm 仅同步状态` |
| 子任务 | SubagentStart 计数>0 | `⑂脉冲 派了 N 个子 Agent 并行干活` |
| 压缩记忆 | PreCompact | `◌旋转 正在压缩记忆` |
| 这轮完成 | Stop | 绿 ✓ + 整行闪绿 1.4s → 转 completion 或空闲 |
| 出错 | StopFailure | 整行抖动一次 → `出错停了 · 等你处理` |
| 空闲 | 无活动会话 | `空闲 · 没有任务` |

**Gemini 诚实降级**：只有 BeforeAgent/AfterAgent → 活动行恒为 `柔和脉冲点 干活中`，hover/点按提示"Gemini 仅提供轮级事件"。**Codex 未开启 PreToolUse 可选项时同样降级为轮级**（thinking↔done）。

### 12.3 进度条（严格条件展示）

唯一合法数据源：Claude 的 TaskCreated/TaskCompleted → `plan_done/plan_total`，显示 `第 n/m 步` + 真实百分比。**没有该数据一律不显示进度条**（禁止时间估算假进度——诚实原则）。

### 12.4 渲染工程要求

动画 tick（600ms）只更新文本与计时节点，**不重建 DOM**（保证 CSS 动画连续——原型已验证的方案）；状态切换时活动行 160ms 淡出淡入；WS 的 sessions_changed 才触发行级重建。

`activity_since` 由 Runtime 随快照提供；前端每秒只更新计时文本。Runtime 每
30 秒执行存活性协调，快照再次执行 30 分钟可见性过滤，但任何仍有 open / committing /
decision_sent / snoozed 事项的会话都必须保留。

---

## 13. 模块三：额度

数据源：`quota[]`。v1 采集方式（照抄已验证方案 + 诚实降级）：

| Provider | 采集 | 依据 |
|---|---|---|
| Claude | 安装可选的 statusline bridge（写入 `~/.actrealm/bin/statusline`，缓存 rate_limits 到 `~/.actrealm/cache/claude-rl.json`），读 5h/7d 窗口 | Open Vibe Island 验证；无自定义项时直接接入；已有自定义项时只在用户显式选择“保留现有并开启”后保存原对象、代理原输出，卸载逐字段恢复，绝不静默覆盖 |
| Codex | **P1/实验性**：解析最近 rollout 中明确识别的 rate-limit 记录（只读、5 分钟轮询） | 内部格式不稳定；独立 adapter + fixture 版本门禁，当前接受桌面 0.144.2、CLI 0.144.4/0.144.5，未知 schema 立即不可用，不读取对话内容 |
| Gemini | 不采集 | 无可靠来源 |

### 13.1 展示

- 固定三行且顺序不漂移：`Claude · 5 小时`、`Claude · 7 天`、
  `Codex · 本周`；每行包含百分比进度条（≥50% 绿 / 20–50% 橙 /
  <20% 红）+ `周几 HH:MM 重置`；
- **数据不可用是一等状态**：显示灰条 + "暂无数据（<原因：未安装桥 / 文件不存在 / 解析失败>）"，附"如何开启"入口——绝不显示旧数据冒充实时（快照超过 30 分钟标注"N 分钟前"）；
- 常规轮询仍为 5 分钟，但 Claude cache 从不存在变为存在或文件修改时间变化时
  立即失效旧快照，避免首次响应后仍显示五分钟“不可用”；
- 今日成本：v1 **不做**美元估算（无可靠单价与完整 token 数据）——只显示"今日事件数 / 会话数"作为活跃度替代。此处与原型不同，原型的 $3.82 是假数据，正式版禁止虚构。

---

## 14. Widget 网格引擎规格（v1.1，不阻塞 v1 发布）

v1 固定三栏，不交付拖动、缩放与模块显隐。以下规格保留为 v1.1 候选，必须另补属性测试证明“无重叠、无越界、无空洞”后才能启用；`bento-touch.html` 只是原型，不视为算法正确性证明：

- 虚拟网格 12 列 × 6 行，模块 = `{x,y,w,h,minW,minH}`；
- **拖动**：模块标题栏为手柄（Pointer Events + setPointerCapture，touch-action:none）；拖动中显示吸附占位框，紫=可放红=不可；
- **缩放**：右下角手柄，实时预览，受 min 约束；
- **碰撞**：目标位置重叠者向下推（BFS，越界=失败回弹）；
- **补位**：任何布局变更后执行 `compact`（全体上浮）+ `fill`（先纵后横拉伸吃空格，两轮）——保证任意时刻无空洞、铺满全屏；
- **限制**：待处理模块 min 4×4 且不可隐藏（锁定）；Agent 任务 min 3×2；额度 min 2×2；
- 布局在 pointerup 后 PUT /settings 持久化，启动时恢复；恢复失败（如模块集变化）回默认布局；
- 编辑模式（显隐开关）下模块内容 pointer-events:none 防误触。

---

## 15. 通知与设置

设置面板（顶栏齿轮，一屏放完）：

| 项 | 选项 | 默认 |
|---|---|---|
| Agent 等待批准 | 弹窗 / 仅列表 / 忽略 | 弹窗 |
| 批准接管超时 | Provider 固定值（只读）：Claude 24h / Codex 1h | 到期后交还终端 |
| Agent 提问 | 同上 | 弹窗 |
| 出错 / 卡住 | 同上 | 弹窗 |
| 任务完成 | 同上 | 仅列表 |
| 提示音 | 开/关 | 开（短促系统音） |
| 按 Agent 静音 | 每 Provider 开关 | 全开 |
| Codex 工具级事件 | 开/关（重装 hook） | 首次运行界面开；CLI 安装默认关 |
| 事件保留天数 | 30/90/365 | 90 |
| 数据导出 / 彻底清除 | 按钮 | — |

"弹窗"=页内横幅 + 提示音（本产品自身就是常驻通知面，不做系统通知——菜单栏薄壳属 v2）。所有设置即时生效、写 settings 表。

---

## 16. Onboarding 与异常态

首次启动（无任何 Provider 接入）待处理模块位置显示引导流：

```text
第 1 步  检测：扫描 claude/codex/gemini CLI（PATH+常见路径，各 2s 超时）
        → 列出检测结果卡（已装/未装）
第 2 步  安装 Hook：每个已装 Provider 一个「接入」按钮
        → 点击显示将写入的配置 diff（原文高亮）→ 用户确认 → 执行 §5 安装
        → 对 Claude/Codex 明示：“权限提示会先出现在 ActRealm；Claude 最长等待
          24 小时、Codex 最长等待 1 小时；连接中断或点击‘去终端处理’会立即交还原终端”
          → 用户单独确认
        → Codex 额外显示："请在 Codex 里运行 /hooks 并信任 actrealm 条目"
          + 「检查信任状态」按钮（doctor 探测）
第 3 步  验证：真实测试事件依次走通 observe → approve/deny → pass-through
        → Agent 任务模块该行亮起并显示实际 capability
        → 显示"完成 ✓ 去终端里正常使用你的 Agent 即可"
```

验收标准：一个装了 Claude Code 的新用户 ≤3 分钟完成接入并看到第一个实时事件。

异常态清单（每个都要实现）：Runtime 离线（§10.2）、hook 安装了但 10 分钟无事件、Provider 版本探测失败、Codex 已安装未信任、批准 waiter 已失效、决定已发送但未确认、pass-through 后无法聚焦终端。批准写回失败时：若 waiter 仍活动则恢复 open；否则 expired 并提示“已交还原终端”，不得恢复假卡片。

---

## 17. 安全与隐私要求

- HTTP 只绑 `127.0.0.1`，随机端口；`serve --open` 生成一次性 bootstrap token（URL fragment），前端 POST 换 HttpOnly+SameSite=Strict Cookie 与 CSRF token 后清除 fragment；所有 mutation 校验 Origin/Host/CSRF，WebSocket 单独认证；无 CORS 放开；
- `~/.actrealm/run` 目录 0700，Unix socket 0600；可用时校验 peer uid；Hook 输入上限 256KB、Bridge 完整帧上限 320KB、本地 API 请求体上限 64KB；JSON 深度上限 32；
- **默认不持久化**：prompt 全文、完整命令、tool input/output、文件内容。活动 PermissionRequest 的完整命令只保存在 waiter 内存中，结束后立即释放；SQLite 只存类别化 command_preview。events.summary 生成时执行脱敏；诊断模式默认关闭、仅能显式开启 1-60 分钟，只记录固定事件类别、Provider、时间、是否需回复和 payload 字节数，不记录原始 payload/session/路径/prompt/命令/参数/URL/token，到期自动清除且失败不得阻塞 Agent；
- 风险提示器绝不绕过 Provider deny/ask、企业策略或沙箱；v1 不实现 always-allow；
- 所有 Provider 文本、命令摘要和错误只能用 `textContent`/安全模板渲染，禁止把 Hook 数据拼进 `innerHTML`；控制字符与 ANSI 在入库和投影时清理；
- 日志脱敏同上；无遥测、无自动更新检查、无任何出站网络请求（CI 断言）；
- 跳回终端的 osascript 需要辅助功能/自动化权限：首次使用引导授权，拒绝则降级为"已复制 cd 命令"提示；
- `export` 输出全部本地数据 JSON；"彻底清除"删 data.sqlite + cache + spool 并要求输入 DELETE 确认。

---

## 18. 工程结构与构建

```text
actrealm/
├── Cargo.toml                    # workspace
├── crates/
│   ├── core/          # 状态机、attention 规则、风险提示器（纯逻辑，无 IO，关键分支全覆盖）
│   ├── providers/     # claude/codex/gemini 的 payload 解析 + 规范化 + fixtures
│   ├── runtime/       # rusqlite 单 writer、迁移、waiter、spool、单实例
│   ├── bridge/        # unix socket 服务端/客户端与版本化帧
│   ├── server/        # axum HTTP+WS、静态资源(rust-embed)、命令延迟提交
│   └── app/           # main.rs：clap 子命令、单实例、组装
├── web/               # index.html + app.css + app.js（无构建，直接嵌入）
│   └── vendor/        # 无外部依赖，本目录应为空——禁止 CDN
├── hooks-config/      # 各 provider 写入模板 + risk_rules.toml
├── fixtures/          # 各 provider 真实脱敏事件样本（按 provider/版本分目录）
└── tests/             # 集成测试：伪 hook 进程 → socket → API 断言
```

构建：`cargo build --release` 产出单二进制（web/ 经 rust-embed 打入）。目标平台 v1：macOS arm64/x64。开发运行：`cargo run -- serve --open --dev`（--dev 从磁盘读 web/ 便于改样式热刷新）。

---

## 19. 开发里程碑与验收标准

按顺序实现，**每个里程碑必须先写验收测试再写实现**。

**M0 · 链路穿刺（最高风险，最先做）**
交付：Claude + Codex 两个真实 Hook probe、最小 socket 服务、Provider 版本化 fixtures、Capability Matrix、接入边界报告。此阶段允许用终端 y/n 代替 Web UI，但必须使用与正式版相同的 requestId/reply frame。

验收（Claude 与 Codex 分别跑完）：
1. Session/Prompt/Tool/Stop 事件能形成基本状态；
2. PermissionRequest 能 approve、deny，并观察到 Agent 后续证据；
3. 3 秒窗口内 undo 后没有任何决定写给 Provider；
4. `pass_through` 后 Provider 原生批准提示可用；
5. hard deadline 后自动 pass-through（测试可注入 3 秒 deadline）；
6. Runtime 未启动时 ≤200ms fail-open；等待中 kill Runtime 后 Hook 立即退出、原终端可继续；
7. 另一个 Hook 返回 deny 时，ActRealm 不把自己的 allow 标成 confirmed；
8. Codex 未信任时明确显示未接入，信任后真实事件走通。

**M0 任一 P0 Provider 不过，项目暂停重估，禁止先写完整 Runtime/UI 绕过风险。**

**M1 · Runtime 核心**
交付：core 状态机 + storage + requestId/correlation-key waiter registry + attention 生成 + 命令延迟提交（含 undo/pass-through）+ 单实例 + 有界非权限事件 spool。
验收：fixtures 回放全绿；同一 envelope 重放幂等；approve/deny/pass-through 竞争只有一个赢家；重复权限请求安全替换旧 waiter；socket half-close 不得被当成用户断线或自动 deny；Runtime 重启后无 waiter 的批准卡全部 expired，绝不恢复为可操作 open；Stop 只表示轮结束，会话退出由进程存活与超时协调降级。

**M2 · API + 最小 UI**
交付：axum snapshot/ws/commands + 三模块静态布局（无拖拽）+ 待处理卡全交互。
验收：真实 Claude 与 Codex 均走通“出卡→批准→撤回→拒绝→去终端处理→超时交回”；UI 明确区分 pending_commit / decision_sent / confirmed / expired。

**M3 · 安装器 + Onboarding + 实时活动**
交付：Claude/Codex 安装卸载、备份/语义合并、Codex 信任引导、doctor、Agent 任务模块事实状态与降级。
验收：≤3 分钟完成接入；install→uninstall 后用户原配置语义不变；升级 Hook 导致哈希变化时能准确提示重新信任；未知字段/事件不崩溃。

**M4 · 额度 + 设置 + P1 Provider**
交付：额度采集双通道与不可用态、通知规则、数据管理；Gemini 轮级观察为 P1，不能阻塞 v1 发布。
验收：拔掉 statusline bridge/删除或改变 rollout schema 后 UI 呈现诚实降级；无可靠数据时不展示百分比。网格引擎不在 v1 发布门槛内。

**M5 · 打磨与验证埋点（并行发布门禁，尚未全部完成）**
交付：§21 埋点、导出、性能达标、异常态全覆盖。
验收：§20 短时性能预算已通过；连续运行 48h Runtime RSS 稳定在 <80MB 仍待完成；所有 pass-through 路径均能继续使用原界面。M6-M13 完成不等于此发布门禁自动完成。

**M6 · 实时任务与待处理联动**
交付：活跃/待处理/近 30 分钟会话过滤、待处理反向定位任务卡、事实活动态、真实 Provider 图标。
验收：老会话有待处理时不被过滤；点击待处理能置顶、高亮并滚动到对应任务；计时 tick 不重建整行；缺少细粒度事件时诚实降级。

**M7 · 动态额度与事实时间**
交付：动态额度窗口、上次有效值保留、Claude cache 即时失效、statusLine 安全 wrapper、本轮总时间与阶段时间。
验收：不硬编码“本周”；未知 schema 不显示假额度；30 分钟后仍显示真实旧值与采样时间；已有 statusLine 原样恢复；时间与 Runtime 事实一致。

**M8 · Desktop 接入与真实控制边界**
交付：无全局 CLI 的桌面 App 发现、Provider 处理后的状态同步、安全忽略、四级跳转、重启恢复与 Token 来源约束。
验收：桌面-only 安装/Doctor 通过；Codex 信任仍由用户执行；Provider 可靠终态清除旧 `等你`；跳转能力不夸大；重启不恢复断开的 waiter。

**M9 · Provider 会话标题一致性**
交付：Claude 官方/custom/AI 标题、Codex thread_name、标题来源优先级与限量读取、三行标题栈。
验收：主标题与客户端标题一致；第二行直接写限长任务，第三行只写模型；新标题约 2 秒内更新；迁移不丢会话；transcript 与路径不进入浏览器。

**M10 · 可配置安全展示**
交付：简洁/详细/开发者三档、安全字段目录、任务卡字段选择、详情抽屉和设置持久化。
验收：未知/raw/payload/full-command/transcript 字段均被服务端拒绝，刷新和 Runtime 重启后设置仍在。

**M11 · Agent 问题直接回答**
交付：Claude AskUserQuestion、Elicitation、秘密字段保护；只有明确托管的 Codex requestUserInput 才展示回答框。
验收：答案只经过 DOM、认证请求、内存 waiter 和一次 Provider 回复；不写 SQLite、日志、诊断或导出；过期问题不能提交。

**M12 · Codex Connector 与会话恢复**
交付：显式 app-server attach、thread/list/resume、requestUserInput、五级恢复状态、进程存活协调。
验收：只有真实 resume 后显示可控制；External Hook 只显示观察能力；Runtime 重启重新连接 managed Thread，但全部旧 waiter 先失效。

**M13 · Provider 原生批准状态协同（自动门禁已过，真实 Provider 复验待完成）**
交付：auto-review ownership、Codex request_permissions 生命周期、managed waitingOnApproval、原生/ActRealm 可回复批准去重、通知与任务状态同步。
验收：原生请求没有 requestId 和允许/拒绝按钮；明确 waiting 信号不被普通 running 覆盖；明确 resolution 后中性消退；不推断批准/拒绝/执行。五轮进程回放、153 测试、Clippy、格式、release、2 分钟资源和隔离浏览器 QA 已通过；真实 Provider 最终复验仍待记录。

**M14 · 实时用量、上下文、价格与 OAuth 额度（已完成本机验收）**
交付：Claude/Codex 会话累计与本轮 Token、真实当前上下文占用、缓存/推理拆分、明确标注的估算 API 价、Claude OAuth 动态额度（含 Fable/额外用量）、一分钟后台刷新、旧值持续展示和凭据零持久化。
验收：Claude 流式消息和子 Agent 记录不重复计数；Codex cache/reasoning 不重复计数；上下文不使用 lifetime Token 冒充；未知模型不显示假价格；OAuth 不阻塞 UI 且 token 不进入 SQLite/缓存/日志/argv；161 项测试、Clippy、格式、release、2 分钟资源门禁、精确 release 安装和本机用户验收均已通过。

**M15 · Codex managed approvals（候选，未实现）**
拟议范围：version-gated `item/commandExecution/requestApproval`、`item/fileChange/requestApproval`、`item/permissions/requestApproval`、availableDecisions、响应与 `serverRequest/resolved` 对账。只有显式 attach 且能力握手成功的 Thread 才能显示直接审批；不得承诺控制任意独立 Codex Desktop 会话。

---

## 20. 测试要求

1. **Provider 合约测试**：fixtures 按 provider/version/event 保存真实脱敏样本；Claude PermissionRequest 无 tool_use_id、Codex PermissionRequest 有 turn_id 等差异必须固化；未知字段兼容、缺字段不得 panic；
2. **状态机属性测试**：重复/乱序/迟到事件；同 requestId 的 approve/deny/pass-through 竞争；decision_sent 不得无证据变 confirmed；
3. **崩溃恢复**：事务各阶段 kill -9 Runtime；等待中 socket EOF；重启后旧批准 expired；非 Permission spool 重放不重复；PermissionRequest 永不 spool 重放；
4. **E2E 手册**：接入→批准→撤回→拒绝→主动 pass-through→deadline pass-through→Runtime 崩溃 pass-through→另一 Hook deny→跳回终端→额度降级→重启一致性；
5. **性能预算**：hook 进程 p95 <50ms（非挂起事件全程）；事件→UI 渲染 p95 <300ms；Runtime 空闲 CPU <0.5%；UI 常驻内存（浏览器 tab）<150MB；
6. **安全测试**：超大 payload、深嵌套 JSON、Host/Origin 伪造、socket 被非本用户访问（权限断言）、脱敏正则用例集。

---

## 21. 验证埋点（硬件立项依据）

全部本地统计（metrics_daily 表），设置页可见"我的使用统计"并可一键导出截图/JSON。指标与门槛（20 名种子用户 × 4 周）：

| 指标 | 定义 | 立项门槛 |
|---|---|---|
| 周留存 | 当周有 ≥3 天产生 app_opened | ≥40% |
| 面板处理率 | (widget_approvals + widget_denials) / approval_requests | ≥50% |
| 终端交还率 | (pass_through_manual + pass_through_timeout) / approval_requests | 持续下降且 timeout <20% |
| 日均面板决策数 | (widget_approvals + widget_denials) / 活跃日 | ≥5 |
| 响应时延 | decision_response_ms_total / decision_response_count | 呈下降趋势 |

不再推测“terminal 决策”。V1 只记录可确定事实：面板 approve/deny、用户主动 pass-through、deadline pass-through。交还终端后的最终批准/拒绝无法由稳定 Hook 事件可靠区分时，不生成虚假指标。

由于产品无遥测，种子测试采用用户知情、自愿的周期性本地导出；汇总表只接收指标计数与版本信息，不接收 prompt、命令、路径或事件明细。未提交导出的用户只计为“未知”，不得当作流失样本偷偷推断。

---

## 22. 风险与降级清单

| 风险 | 应对 |
|---|---|
| Provider 升级改 Hook schema | 合约测试 + 未知事件可见化（§7）+ fixtures 按版本归档 |
| Codex 信任流程流失用户 | Onboarding 显式引导 + doctor 可检测"已装未信任" |
| Hook 接管导致原终端暂时不显示批准框 | 明示接管；提供“去终端处理”；Claude 24h / Codex 1h hard deadline 自动 pass-through |
| Provider 原界面已有批准框，但 ActRealm 误显示可审批 | native approval 无 requestId，UI capability 门禁，只显示原界面处理/提醒/忽略；明确 resolution 后中性消退 |
| auto-review 与 ActRealm waiter 竞争 | 识别 Provider approval owner；Provider-owned 路径不创建阻塞 waiter；同 Session Hook/native 卡去重 |
| Runtime 在批准等待中崩溃 | socket EOF → Hook 无决定退出；旧卡 expired；不尝试跨进程恢复 waiter |
| 多 Hook 决策冲突 | stdout 只记 decision_sent；以后续 Provider 事件确认；任何 deny 不被伪装成成功 |
| 风险规则误判 | 明确为非权威提示；测试/构建默认 medium；不改变 Provider 安全策略 |
| 额度数据源失效 | §13 一等不可用态 |
| osascript 权限被拒 | 跳回降级为复制路径提示 |
| 用户配置文件被其他工具并发修改 | 安装器文件锁 + 校验后写 + 失败回滚备份 |
| SQLite 损坏 | 启动 integrity_check，失败则归档旧库重建；活动 Hook 因 Runtime 断开立即 pass-through |
| Codex app-server approval schema/version 不兼容 | M13 只观察 native waiting；M15 三类 managed approval 必须经过显式能力握手、版本门禁和 fixture 后才能显示控制按钮 |

---

## 23. 附录：Hook 配置与应答格式

### 23.1 写入 Claude `~/.claude/settings.json` 的条目（语义合并进 hooks 字段）

```json
{
  "hooks": {
    "PermissionRequest": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/ABSOLUTE/HOME/.actrealm/bin/actrealm hook --provider claude",
            "timeout": 86400
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/ABSOLUTE/HOME/.actrealm/bin/actrealm hook --provider claude",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

安装器写入真实绝对路径，不把 `~` 的展开行为当成跨 Provider 保证。其他观察事件同构，timeout ≤5s。

### 23.2 PermissionRequest 应答与 pass-through

Claude/Codex 允许均使用官方最小形状：
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": { "behavior": "allow" }
  }
}
```
拒绝：
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": {
      "behavior": "deny",
      "message": "User denied via ActRealm"
    }
  }
}
```

pass-through：stdout 保持为空并以成功状态退出。Hook 只能在尚未写出 allow/deny 时 pass-through；禁止先写部分 JSON 再回退。

### 23.3 Codex `~/.codex/hooks.json` 条目（首选）

```json
{
  "hooks": {
    "PermissionRequest": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "/ABSOLUTE/HOME/.actrealm/bin/actrealm hook --provider codex",
            "timeout": 3600,
            "statusMessage": "Waiting for ActRealm approval"
          }
        ]
      }
    ]
  }
}
```

若用户选择 inline `config.toml`，等价格式为：

```toml
[[hooks.PermissionRequest]]
matcher = "*"

[[hooks.PermissionRequest.hooks]]
type = "command"
command = "/ABSOLUTE/HOME/.actrealm/bin/actrealm hook --provider codex"
timeout = 3600
statusMessage = "Waiting for ActRealm approval"
```

Hooks 当前默认启用；安装器不写 deprecated `codex_hooks`。若检测到旧版确实需要 feature flag，只在兼容分支写其支持的键，并由 fixture/doctor 验证。

### 23.4 参考实现（技术已验证来源）

- Hook 事件全集与应答格式：[Open Island docs/hooks.md](https://github.com/Octane0411/open-vibe-island/blob/main/docs/hooks.md)
- Unix socket 中继与稳定 bin 路径：Open Island `OpenIslandHooks` / [CodeIsland bridge](https://github.com/wxtsky/CodeIsland)
- 额度窗口读取（statusline 桥 / rollout 文件）：Open Island README「Connect Claude Code / Codex」
- 官方文档：[Claude Code Hooks](https://code.claude.com/docs/en/hooks) · [Codex Hooks](https://developers.openai.com/codex/hooks)

—— 完 ——
