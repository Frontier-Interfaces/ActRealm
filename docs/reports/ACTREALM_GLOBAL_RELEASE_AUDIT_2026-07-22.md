# ActRealm 全局上线前测试报告

- 测试日期：2026-07-22（Asia/Shanghai）
- 测试对象：`Frontier-Interfaces/ActRealm` 的 `agent/v1-full`
- 冻结候选：`50b891a4e7f7b8c07349d5a082cfc9dcfc9e7bd0`
- 安装位置：`/Applications/ActRealm.app`
- 测试主机：Apple Silicon Mac，macOS 26
- 测试方式：自动化门禁、真实 Hook、原生 macOS UI、故障注入、隔离首装、压力、资源、安全、静态审计
- 结论：**不建议按当前候选公开发布（NO-GO）**

## 1. 执行摘要

ActRealm 的核心地基已经具备较好的可靠性：Rust/Swift 自动化测试全部通过；Runtime 仅监听本机；Socket、数据库和缓存权限正确；Hook 在 Runtime 离线时可以静默放行；真实 Claude/Codex 事件可以进入 Runtime；500 个唯一会话、16 并发的压力测试通过；隔离首装不会绕过 Codex 的官方信任步骤。

但当前候选仍有影响核心承诺的上线阻断问题：

1. 多条待处理事项并存时，Outbox 使用数组下标保存选择，排序或移除后会选错卡片；Claude 提问/表单因此可能完全无法回答。
2. Runtime 崩溃后 12 秒内没有自动拉起；界面在离线状态仍保留失效的批准按钮。
3. App 异常退出后 Runtime 仍存活，但新 App 无法重新认证并接管它；用户看到“另一个实例已运行”却无法使用。
4. Runtime 重启后旧审批虽然已经过期，任务仍显示“等待批准”，形成幽灵等待状态。
5. Codex app-server listener 的生命周期没有由 Connector 持有，主机上已经积累多组 PPID 1 的长期孤儿进程。
6. Agent Focus 在可见和最小化时都持续占用约 19%–27% App CPU；活跃大会话下 Runtime 约 7%–12% CPU。
7. App 仅有 ad-hoc 签名、Gatekeeper 拒绝、只含 arm64、最低 macOS 26，且无安装器、登录启动、更新和回滚机制。
8. GitHub Actions 没有 Rust/Swift 构建测试门禁；48 小时冻结候选浸泡、干净系统、不同硬件/系统和真实睡眠唤醒均未完成。
9. 睡眠唤醒后没有专门的重连和 Claude 额度强制刷新路径；僵死 WebSocket 可能长期保留旧额度。
10. Claude 打开时发出的历史恢复 SessionStart/SessionEnd 被误当成“最近任务”，即使用户没有输入任何内容也会生成空卡片。

因此，本报告建议先完成 P0 修复，再重新冻结候选并执行完整发布门禁。当前版本可继续用于开发验证，但不应向普通新用户承诺“稳定控制 Agent”。

## 2. 测试结果总览

| 领域 | 结果 | 说明 |
|---|---:|---|
| Rust 格式、Clippy、全工作区测试、Release 构建 | 通过 | 零警告 Clippy；完整 Rust 测试通过 |
| Swift 测试 | 通过 | 12 个 Suite、71 项测试通过 |
| RustSec | 通过 | 132 个依赖、1166 条 advisory，未发现命中 |
| 真实 Claude/Codex Hook | 部分通过 | 接入、生命周期、审批、完成事件可用；并发 Outbox 与提问 UI 失败 |
| Codex 审批模式 | 部分通过 | 外部 Hook、Provider 自处理、原生观察可以区分；完全访问文案错误 |
| Runtime 离线放行 | 通过 | 阻塞 Hook 在 Runtime 被杀后成功静默返回 |
| Runtime 自动恢复 | 失败 | Runtime 被杀后没有自动拉起，Socket 残留 |
| App 异常退出恢复 | 失败 | 新 App 无法附着到仍存活的 Runtime |
| 数据库重启恢复 | 部分通过 | 历史保留、完整性正常；审批任务状态未被正确归约 |
| 500 会话 / 16 并发压力 | 通过 | 2.32 秒完成，500 session / 500 event，完整性 `ok` |
| 2 分钟空闲资源门禁 | 通过 | 118 个样本，Runtime CPU 0%，RSS 最大 6288 KiB |
| 活跃资源与最小化 | 失败 | Agent Focus 最小化后仍约 19%–24% App CPU |
| 网络与本地权限 | 通过 | 仅 `127.0.0.1`；Socket 0600；数据目录 0700 |
| 导出隐私 | 基本通过 | JSON 有效，命令内容脱敏，无 OAuth/启动令牌字段 |
| 诊断日志隐私 | 失败 | bootstrap 一次性令牌会先进入可显示的 stdoutTail |
| 隔离首装与卸载 | 通过 | Claude 安装/移除、Codex 信任引导均为真实功能 |
| 签名、公证、Gatekeeper | 失败 | ad-hoc；`spctl` 拒绝 |
| CI 发布门禁 | 失败 | 只有语言守卫和 Pages，没有 Rust/Swift 测试工作流 |
| 键盘/VoiceOver/AX | 未通过 | Tab 12 次焦点不离开窗口；主题页触发辅助功能采集服务崩溃 |
| 48 小时冻结候选浸泡 | 未执行 | 验收文档仍明确未完成，不能记为通过 |
| 干净系统、Intel、多系统、多显示器 | 未执行 | 当前只有一台 Apple Silicon/macOS 26 主机 |
| 真实睡眠/唤醒、系统锁屏、断网恢复 | 未执行 | 会中断当前测试控制，需物理设备矩阵专项执行 |

自动门禁原始记录：[01-automated-gates.log](evidence/2026-07-22/01-automated-gates.log)。

## 3. 问题清单

### AR-001 · P0 · 并发 Outbox 使用数组下标，排序后选择错位

- 层级：macOS 客户端状态管理。
- 影响：用户点击队列中的 Claude/Codex 事项，主卡片可能仍显示另一项；批准、拒绝或回答存在作用到错误对象的认知风险。
- 触发：同时制造 Codex 审批、Claude 审批、Claude AskUserQuestion 和 Claude Elicitation；点击队列中的 Claude 项。`revealSession` 改变排序/置顶后，原数字下标指向另一条记录。
- 实际：界面模型记录的下标发生变化，但主卡片仍是 Codex；移除第一项后，剩余主卡片还会退化为一行队列项。
- 预期：选择应绑定稳定 `attention_id`；重排后仍展示用户点击的同一事项。
- 根因证据：`OutboxSection` 以 `outboxPageIndex` 选取 `entries[selectedIndex]`，点击时先写数字下标再调用可能改变排序的 `revealSession`。源码锚点见 [39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)。
- 现场证据：[03-outbox-four-concurrent.png](evidence/2026-07-22/03-outbox-four-concurrent.png)、[04-outbox-selection-index-bug.png](evidence/2026-07-22/04-outbox-selection-index-bug.png)、[06-outbox-primary-disappears-after-resolution.png](evidence/2026-07-22/06-outbox-primary-disappears-after-resolution.png)。
- 准备改进：把 `outboxPageIndex` 改为 `selectedAttentionID`；每次 snapshot 后按 ID 重新定位；若选中项已解决，选择“最早仍可操作项”，没有则显示空态。
- 复测门槛：2、3、5、20 条混合事项；任意顺序插入、置顶、解决、忽略；操作对象 ID 必须与用户选中 ID 完全一致。

### AR-002 · P0 · Claude 提问和 Elicitation 在原生 Outbox 中无法回答

- 层级：macOS 客户端交互，受 AR-001 选择错位影响。
- 影响：V1 承诺的“Agent 提问直接在待处理栏回答”在真实界面不可用；阻塞 Hook 只能回 Provider 处理或等待超时。
- 触发：发送真实 Claude `AskUserQuestion` 或 Elicitation；清除其他事项后只保留一条提问。
- 实际：Outbox 仅显示一行“提问”，点击后也没有单选、多选、自由输入或提交表单。
- 预期：选中的问题必须渲染 `InteractiveQuestionView`，秘密字段使用安全输入，过期后禁用提交。
- 证据：[07-claude-question-form-missing.png](evidence/2026-07-22/07-claude-question-form-missing.png)。后端回答结构的自动化测试通过，说明缺口主要在原生 UI 投影。
- 准备改进：完成 AR-001 的稳定选择；对 `question`/`elicitation` 类型强制使用交互主卡；加入 1–4 问题、单选、多选、自由输入、秘密字段、超时和重启失效的原生 UI 集成测试。
- 复测门槛：真实 Claude 会话逐类提问，在 ActRealm 中回答后 Provider 获得精确 `updatedInput.answers`；秘密答案不进入数据库、日志、导出。

### AR-003 · P0 · Runtime 崩溃后没有自动重启

- 层级：macOS `RuntimeSupervisor` 生命周期。
- 影响：Runtime 一旦异常退出，Agent 事件、审批和额度同步全部中断，用户必须手动进入设置重启。
- 触发：在真实阻塞 Claude waiter 存在时对 App 管理的 Runtime 发送 `SIGKILL`。
- 实际：12 秒内没有新 Runtime；旧 Socket 文件仍存在但连接被拒绝。Hook 能静默 fail-open，这是正确的安全降级，但监控能力已失效。
- 预期：检测非预期退出后自动退避重启；清理残留 Socket；在达到重试上限后明确告警。
- 根因证据：termination handler 只设置 `.failed`，没有 watchdog 或重启调度，见 [39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)。
- 现场证据：[13-runtime-kill-recovery.log](evidence/2026-07-22/13-runtime-kill-recovery.log)、[40-test-cleanup-and-final-doctor.log](evidence/2026-07-22/40-test-cleanup-and-final-doctor.log)。
- 准备改进：Supervisor 增加“非用户 stop”判定、0.5/1/2/4/8 秒指数退避、连续失败熔断；启动前只删除经过类型/所有者检查的残留 Socket。
- 复测门槛：连续杀死 Runtime 20 次；每次 5 秒内恢复，Hook 永不被卡死，数据库完整，无多实例和残留 Socket。

### AR-004 · P0 · Runtime 离线时仍显示可点击的旧批准按钮

- 层级：macOS 客户端离线投影。
- 影响：用户以为批准仍有效，实际 waiter 已因 Runtime 崩溃失去回复通道；点击可能失败或造成状态误解。
- 触发：待批准事项显示时杀死 Runtime。
- 实际：顶部显示“服务未连接”，旧卡片仍保留批准/拒绝动作，并出现异常长的过期时间。
- 预期：连接断开后立即禁用所有控制；卡片标为“控制通道已失效/仅观察”；允许打开 Provider，但不得继续伪装可批准。
- 证据：[14-runtime-dead-stale-action-buttons.png](evidence/2026-07-22/14-runtime-dead-stale-action-buttons.png)。
- 准备改进：把动作可用性同时绑定 `runtimeConnection == online`、waiter 活性和 attention state；离线 snapshot 使用本地只读缓存并显示来源时间。
- 复测门槛：在审批、问题、Elicitation、完成确认四类卡片上分别断开 Runtime；所有失效动作在 1 个刷新周期内禁用。

### AR-005 · P0 · App 异常退出后无法附着到仍存活的 Runtime

- 层级：App/Runtime 认证与进程所有权。
- 影响：App 崩溃或被强退后，Runtime 仍健康，但重新打开的 App 无法取得 bootstrap/session，主界面无任务、无额度，只能手动重启 Runtime。
- 触发：只终止 ActRealm App 进程，保留它启动的 Runtime；再次启动 App。
- 实际：CLI `doctor` 仍通过，但 App 显示“另一个 actrealm 实例已在运行”并处于未连接状态。
- 预期：App 能安全重新附着到自己拥有的本机 Runtime，或在确认进程路径/用户/锁后平滑替换它。
- 证据：[17-app-relaunch-cannot-attach-existing-runtime.png](evidence/2026-07-22/17-app-relaunch-cannot-attach-existing-runtime.png)。
- 准备改进：建立持久化但私有的 authenticated rendezvous，或让 Runtime 提供只允许同 UID/同安装身份使用的一次性重连；备选方案是在 App 退出时可靠终止其子 Runtime并由新 App 重建。
- 复测门槛：App `SIGKILL`/Force Quit/正常退出各 20 次；重新打开后 5 秒内恢复任务和额度，不出现第二 Runtime。

### AR-006 · P0 · Attention 已结束但 Session 仍是“等待批准/回答”

- 层级：Runtime SQLite reducer / 会话恢复。
- 影响：Outbox 已经没有待办，但 Agent Tasks 或数据库仍显示“等待批准/回答”，用户无法判断 Agent 是否真正被阻塞；重启后还可能恢复出幽灵任务。
- 触发 A：存在内存 waiter 时崩溃并重启 Runtime。触发 B：确认/清除 Claude question 或 Elicitation attention。
- 实际：重启场景中 attention 被正确设为 `expired/runtime_restart`，但 session 仍为 `awaiting_approval`、`approval_owner=widget`、活动“等待你批准”。问题场景中 attention 已为 `resolved/ack`，两个 session 仍为 `awaiting_approval/widget/等待你回答`。
- 预期：不恢复旧 stdout waiter；同时把会话归约为“控制已丢失/等待 Provider 新事件/仅观察”，清除 widget 所有权。
- 根因证据：重启清理只更新 attention 和 command，没有同步 sessions，源码见 [39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)。
- 现场证据：[15-restart-ghost-waiting-task.png](evidence/2026-07-22/15-restart-ghost-waiting-task.png)、[41-synthetic-session-final-states.log](evidence/2026-07-22/41-synthetic-session-final-states.log)。
- 准备改进：所有 attention 终态都在同一 SQLite 事务中更新 attention、command 和 session；清理 `approval_owner`，并根据 Provider 活性写入 `thinking`、`waiting_for_event`、`lost_control` 或终态。
- 复测门槛：在四类 waiter 中分别批准、拒绝、忽略、确认、超时和重启；attention 终态后 session 不得继续显示可控制的等待态；下一条 Provider 事件可正常恢复。

### AR-007 · P1 · `decision_sent` 后按钮仍显示可操作

- 层级：macOS 客户端命令状态反馈。
- 影响：用户可能重复点击批准/拒绝；Runtime 能保证只赢一次，但 UI 没有表达“决定已发送，等待 Provider 确认”。
- 触发：在 ActRealm 中批准请求，但尚未收到匹配的 PostToolUse。
- 实际：数据库已是 `decision_sent`，按钮仍存在。
- 预期：按钮立即变为禁用状态和进度文案；只保留撤销（若协议允许且仍在窗口内）。
- 证据：[05-decision-sent-still-actionable.png](evidence/2026-07-22/05-decision-sent-still-actionable.png)。
- 准备改进：动作渲染显式区分 `open`、`pending_commit`、`decision_sent`、`resolved`、`expired`。
- 复测门槛：快速连点、双窗口、API 与 Provider 竞争决定；最多产生一个有效回复。

### AR-008 · P0 · Codex app-server listener 形成长期孤儿进程

- 层级：Rust Codex Connector 进程生命周期。
- 影响：长时间使用后积累多个 `codex app-server --listen` 和子进程，持续占用内存、文件描述符和 Socket；升级后也可能连接到旧版本 listener。
- 触发：多次启动不同候选、Runtime 或 Connector。
- 实际：发现多组存活 1–5 天、PPID 1 的进程；单组约 20–30 MB RSS。当前 `ensure_app_server` 在 Socket ready 后把 child 移入 detached wait thread，Connector Drop 无法 kill 它。
- 证据：[18-process-tree.log](evidence/2026-07-22/18-process-tree.log)；源码 [39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)。
- 准备改进：由专门 owner 保存 listener child；记录二进制路径、版本、PID；最后一个 Connector/Runtime 退出时优雅停止；连接已有 Socket 前验证协议与版本；异常遗留只在确认归属后清理。
- 复测门槛：启动/退出/升级 100 次，最终只允许一个受管 listener；Runtime 完全退出后进程树归零。

### AR-009 · P0 · Agent Focus 最小化后仍持续高 CPU

- 层级：SwiftUI 动画/媒体生命周期。
- 影响：后台耗电、发热，笔记本续航下降；用户即使最小化窗口也继续支付渲染成本。
- 触发：打开 Agent Focus 后最小化窗口。
- 实际：可见时 App 约 15.7%–26.8% CPU；最小化后仍约 18.8%–23.7%。源码使用 30 FPS `TimelineView`，只在 Reduce Motion 或 snapshot 模式暂停。
- 预期：窗口 occluded、miniaturized、App inactive、屏幕锁定时动画和视频完全暂停；静态页面接近空闲 CPU。
- 证据：[21-agent-focus-resource-samples.log](evidence/2026-07-22/21-agent-focus-resource-samples.log)、[22-agent-focus-page.png](evidence/2026-07-22/22-agent-focus-page.png)、[23-minimized-resource-samples.log](evidence/2026-07-22/23-minimized-resource-samples.log)、源码 [39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)。
- 准备改进：接入 scene/window occlusion；只有选中的卡片动画；目标 10–15 FPS 或基于事件的静态过渡；视频播放器随可见性暂停。
- 复测门槛：可见、被遮挡、最小化、锁屏各 10 分钟；最小化 App CPU p95 < 0.5%。

### AR-010 · P1 · 活跃大会话下 Runtime 持续高频读取 SQLite

- 层级：Runtime snapshot / SQLite 查询。
- 影响：大 rollout 和高频事件下 Runtime 约 7%–12% CPU；会随会话、计划和子 Agent 数据增长扩大。
- 触发：打开当前长生命周期 Codex 会话并保持事件流。
- 实际：采样栈集中在重复 `read_snapshot`、`read_plan_steps`、SQL prepare/step；空闲 2 分钟资源门禁仍通过，说明现有门禁没有覆盖活跃负载。
- 预期：事件合并后增量推送；查询复用 prepared statements；未变化时不重建全量 snapshot。
- 证据：[19-main-live-resource-samples.log](evidence/2026-07-22/19-main-live-resource-samples.log)、[20-runtime-high-cpu.sample.txt](evidence/2026-07-22/20-runtime-high-cpu.sample.txt)、空闲对照 [25-two-minute-resource-gate.log](evidence/2026-07-22/25-two-minute-resource-gate.log)。
- 准备改进：snapshot coalescing/debounce、revision/delta API、单事务批量读取、缓存计划/子 Agent 投影；资源门禁改为统计整个进程树和真实大数据集。
- 复测门槛：1、10、100 活跃会话和 1 GB rollout；Runtime CPU/RSS 在验收预算内，UI 延迟 p95 仍小于 300 ms。

### AR-011 · P1 · 完成确认后的 Toast 文案错误

- 层级：macOS 客户端用户反馈。
- 影响：用户点击“确认完成”后却看到“已交回 Codex 的请求 / Provider 后续事件已确认继续”，无法确认自己完成了什么动作。
- 触发：收到 Stop/完成通知后点击“确认完成”。
- 实际：使用了审批/交回类通用文案。
- 预期：明确显示“已确认完成”，并包含对应任务标题。
- 证据：[08-completion-notification.png](evidence/2026-07-22/08-completion-notification.png)、[09-completion-confirmation-wrong-toast.png](evidence/2026-07-22/09-completion-confirmation-wrong-toast.png)。
- 准备改进：按 attention kind 提供语义化结果文案，不复用审批 Toast。
- 复测门槛：审批、拒绝、忽略、问题回答、完成确认分别断言独立文案。

### AR-012 · P1 · 完全访问模式被标成“Codex 正在自动审批”

- 层级：Runtime/客户端状态语义。
- 影响：把“Provider 自动审查”和“完全访问、不再询问”混为一谈，用户会错误理解安全边界。
- 触发：Codex `permission_mode=danger-full-access` 运行工具。
- 实际：与 auto-review 显示同一文案。
- 预期：显示事实性的“完全访问模式，由 Codex 处理”；只有 guardian/auto-review 才使用自动审批文案。
- 证据：[10-provider-owned-modes.log](evidence/2026-07-22/10-provider-owned-modes.log)、[12-provider-owned-completion-notifications.png](evidence/2026-07-22/12-provider-owned-completion-notifications.png)。
- 准备改进：把 `approval_owner` 与 `permission_mode` 分开投影；状态文案只由真实 capability 决定。
- 复测门槛：default、request-keyed、auto-review、full-access、native approval 五种模式逐项快照断言。

### AR-013 · P2 · 根会话 SessionEnd 被描述为“子 Agent 已结束”

- 层级：Runtime 活动文案 reducer。
- 影响：主任务结束和子 Agent 结束混淆，详情时间线失真。
- 触发：向根会话发送 SessionEnd。
- 实际：活动文本为“子 Agent 已结束”。
- 预期：根会话显示“会话已结束”；只有带真实子 Agent 标识的事件才显示子 Agent 文案。
- 证据：真实最终状态 `response_finished / 子 Agent 已结束` 见 [41-synthetic-session-final-states.log](evidence/2026-07-22/41-synthetic-session-final-states.log)。
- 准备改进：文案映射增加 root/subagent 上下文，不按事件名孤立判断。
- 复测门槛：根会话、Task 子 Agent、Codex managed subagent 各自结束，标题和计数一致。

### AR-014 · P1 · 子 Agent 表中存在“running 但 active=false、父会话 idle”的陈旧状态

- 层级：Runtime 子 Agent 持久化语义。
- 影响：当前 UI 用 `active` 计数时仍显示 0，因此暂不直接误报；但详情、开发者导出或后续按 `status` 查询会错误显示仍在运行。
- 触发：历史子 Agent 已停止、父会话已 idle 后检查数据库。
- 实际：5 条记录中 4 条为 `status=running, active=0, parent=idle`。
- 预期：停止时 `active=0` 且 `status=completed/failed/cancelled/unknown`；父会话结束应收敛所有活跃子 Agent。
- 证据：[34-plan-subagent-ci-autostart.log](evidence/2026-07-22/34-plan-subagent-ci-autostart.log)。
- 准备改进：统一 active 与 status 状态机；SessionEnd 事务内关闭仍活跃子 Agent；迁移历史不一致数据。
- 复测门槛：父会话结束、Runtime 重启、异常中断后三种场景中，active/status 不出现矛盾组合。

### AR-015 · P1 · bootstrap 一次性令牌进入可见诊断日志

- 层级：macOS 本机认证与诊断隐私。
- 影响：令牌通常很快被消费，但在消费前后都可能显示在“诊断详情” stdoutTail；截图、客服日志或肩窥会扩大暴露面。
- 触发：App 启动 Runtime，打开诊断详情。
- 实际：Supervisor 先把完整 stdout 行加入最近 4000 字符，再调用 `consume` 解析 `/#bootstrap=<token>`；监控页面直接显示 stdoutTail。
- 预期：任何日志缓存之前先识别并删除/掩码 token；诊断只显示 endpoint 和“bootstrap 已接收”。
- 证据：[33-bootstrap-diagnostics-source.log](evidence/2026-07-22/33-bootstrap-diagnostics-source.log)。导出本身未包含 token，对照见 [32-live-export-privacy-summary.log](evidence/2026-07-22/32-live-export-privacy-summary.log)。
- 准备改进：`consume` 先于 tail；或使用结构化 pipe/文件描述符传递凭证；统一 secret redactor 覆盖 stdout、stderr、诊断和崩溃报告。
- 复测门槛：注入 token、OAuth、Authorization、路径秘密样本，UI/导出/日志/截图文本均无明文。

### AR-016 · P1 · 键盘和辅助功能兼容性未达到上线要求

- 层级：macOS Accessibility/keyboard UX。
- 影响：依赖键盘、VoiceOver 或自动化辅助技术的用户可能无法操作设置、任务和 Outbox；也阻碍可靠 UI 回归测试。
- 触发 A：主窗口连续按 Tab 12 次。
- 实际 A：焦点始终停在窗口容器。该结果可能受系统 Full Keyboard Access 设置影响，因此必须在受控开启环境复测，但当前不能判通过。
- 触发 B：辅助功能服务读取主题设置。
- 实际 B：`SkyComputerUseService` 发生 `EXC_BREAKPOINT/SIGTRAP`；ActRealm 进程没有崩溃。说明是辅助功能交互兼容风险，不是普通用户点击主题必现崩溃。
- 预期：主操作存在稳定 Tab 顺序和 VoiceOver 标签；辅助技术读取主题页不应触发 AX 客户端崩溃。
- 证据：[16-SkyComputerUseService-theme-tab-crash.ips](evidence/2026-07-22/16-SkyComputerUseService-theme-tab-crash.ips)、[38-keyboard-tab-focus.json](evidence/2026-07-22/38-keyboard-tab-focus.json)。数据设置中“使用统计”容器也没有可读子项，截图见 [35-settings-data-accessibility.jpeg](evidence/2026-07-22/35-settings-data-accessibility.jpeg)。
- 准备改进：为自定义卡片、策略选项和统计值提供 Button/RadioGroup/Label 语义、焦点顺序和可访问值；建立 VoiceOver、Keyboard Navigation、Reduce Motion 测试矩阵。
- 复测门槛：仅键盘完成首装、审批、问题回答、设置和导出；Accessibility Inspector 无严重错误；VoiceOver 全流程通过。

### AR-017 · P1 · 已批准范围发生产品功能回归：HUD 与 Agent Focus 仍存在

- 层级：产品范围/UI。
- 影响：用户此前明确要求“通知与数据”只保留“不提醒/仅列表”，去掉 HUD 胶囊和系统通知，并且不需要台前调度；当前候选重新出现 HUD 与 Agent Focus，导致界面和行为偏离已验收设计，也引入 AR-009 的资源问题。
- 触发：打开通知设置或点击主窗口“智能聚焦”。
- 实际：通知设置包含完整 HUD 开关/显示器/时长/测试胶囊；主窗口空态仍写“新事件会先以 HUD 胶囊出现”；Agent Focus 页面可用。
- 预期：按当前产品决定从主流程移除或用明确实验特性门禁隐藏；默认不运行相关动画。
- 证据：[22-agent-focus-page.png](evidence/2026-07-22/22-agent-focus-page.png)、[36-settings-notification-scope-regression.jpeg](evidence/2026-07-22/36-settings-notification-scope-regression.jpeg)。
- 准备改进：先由产品确认是删除、隐藏还是实验开关；默认构建不展示、不启动、不消耗资源，保留代码供后续版本使用。
- 复测门槛：主界面、设置、首装、通知行为与批准设计逐页对照，不出现未批准入口。

### AR-018 · P0 · 发布包未达到普通用户可安装标准

- 层级：分发/兼容性。
- 影响：Gatekeeper 会拒绝；用户需要手动绕过安全检查；Intel Mac 无法运行；低于 macOS 26 的机器无法安装；版本 `0.1.0 (1)` 不能追溯到具体 commit。
- 触发：对 exact candidate 打包并执行 codesign/spctl/架构/Plist 检查。
- 实际：deep codesign 验证通过但为 ad-hoc；`spctl --assess` rejected；App/Helper 仅 arm64；最低系统 26.0；无 Team ID、公证、DMG 和构建 SHA。
- 预期：Developer ID 签名、公证、staple、DMG；明确支持矩阵；版本/tag/commit 可追溯。
- 证据：[24-package-distribution.log](evidence/2026-07-22/24-package-distribution.log)、源码版本锚点 [39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)。
- 准备改进：建立可复现 release pipeline；决定 universal2 或明确 Apple Silicon-only；重新评估最低 macOS；嵌入 commit SHA/build date；执行 clean Mac Gatekeeper 安装。
- 复测门槛：全新用户账户/全新 VM 下载 DMG 后双击安装和首次启动，无安全绕过；`spctl` accepted；签名链和公证票据有效。

### AR-019 · P0 · GitHub CI 没有核心构建和测试门禁

- 层级：工程发布流程。
- 影响：本机门禁虽通过，但任何 push/PR 都可能在没有 Rust/Swift 编译、测试、Clippy、RustSec、打包验证的情况下合并，无法保证远端分支持续可发布。
- 触发：检查 `.github/workflows`。
- 实际：只有 language guard 和 GitHub Pages deployment。
- 预期：PR 必须跑 Rust/Swift 全门禁；release job 绑定冻结 commit，生成签名产物和 checksum。
- 证据：[34-plan-subagent-ci-autostart.log](evidence/2026-07-22/34-plan-subagent-ci-autostart.log)。
- 准备改进：新增 macOS CI：fmt、Clippy `-D warnings`、workspace test、release build、Swift test、plist/package、cargo audit；缓存依赖但不缓存最终二进制；分支保护强制通过。
- 复测门槛：故意提交格式错误、Rust 测试失败、Swift 测试失败、漏洞依赖，CI 必须逐项阻断。

### AR-020 · P1 · 缺少开机启动、自动更新和回滚

- 层级：产品生命周期/运维。
- 影响：新用户重启电脑后不知道如何恢复监控；安全修复无法自动送达；坏版本没有一键回退。
- 触发：静态搜索 App/Runtime 生命周期实现和发布文档。
- 实际：没有 `SMAppService`、Sparkle/Updater 或 rollback 实现；旧 LaunchAgent 仅被识别为过期并移除。
- 预期：可选“登录时启动”；签名更新源；失败回滚；版本迁移备份。
- 证据：[34-plan-subagent-ci-autostart.log](evidence/2026-07-22/34-plan-subagent-ci-autostart.log)。
- 准备改进：使用 `SMAppService.mainApp` 做用户可控登录启动；选择经过签名验证的更新框架；每次升级前备份 schema/config，启动失败自动回滚。
- 复测门槛：重启、升级、降级、断网升级、坏包、数据库迁移失败全流程演练。

### AR-021 · Gate Open · 48 小时冻结候选浸泡尚未执行

- 层级：验收流程，不是单一代码缺陷。
- 影响：无法证明内存、事件数据库、文件句柄、孤儿进程和额度轮询在长时间运行下稳定；短测不能代替长测。
- 触发：核对 `docs/V1_ACCEPTANCE.md` 和 exact candidate。
- 实际：验收项仍是未勾选；本轮只执行了 2 分钟空闲门禁、活跃采样和 500 会话突发。
- 预期：同一个冻结 commit/签名包连续运行 48 小时，期间真实 Claude/Codex 会话、睡眠唤醒、网络波动和 Runtime 重启均有样本。
- 证据：[39-source-anchors.log](evidence/2026-07-22/39-source-anchors.log)、短测 [25-two-minute-resource-gate.log](evidence/2026-07-22/25-two-minute-resource-gate.log)。
- 准备改进：修完 P0 后重新冻结，不在浸泡期间换包；采集整个进程树 CPU/RSS/FD/DB/WAL/事件延迟；超过阈值自动失败并保留时间序列。
- 复测门槛：48 小时完成且无重启泄漏、无持续增长、无卡死，Runtime RSS 全程小于验收上限。

### AR-022 · Gate Open · 兼容性和物理环境矩阵不完整

- 层级：发布验证。
- 影响：当前结果只代表一台 Apple Silicon/macOS 26 主机，不能外推到干净 Mac、Intel、不同系统、多个显示器、真实锁屏/睡眠或企业权限策略。
- 触发：对比预期支持范围和现有测试环境。
- 实际：没有可用的第二硬件/VM；为了不让当前远程测试会话失联，本轮没有强制系统睡眠，也没有关闭系统网络。
- 预期：在声明支持的每个 OS/架构上完成首装、升级、审批、额度、睡眠唤醒、锁屏、断网和卸载。
- 证据：打包支持范围见 [24-package-distribution.log](evidence/2026-07-22/24-package-distribution.log)；隔离首装证据见 [26-first-run-native.png](evidence/2026-07-22/26-first-run-native.png)、[27-first-run-codex-trust-step.png](evidence/2026-07-22/27-first-run-codex-trust-step.png)。
- 准备改进：建立 clean VM 与至少一台真实第二设备；先明确最低 macOS 和架构，再按矩阵执行。
- 复测门槛：所有声明支持组合通过；未测组合不得出现在公开兼容性声明中。

### AR-023 · P1 · 睡眠唤醒后 Claude 额度可能长期不刷新

- 层级：macOS 生命周期、WebSocket 健康检查和 Runtime quota 调度。
- 影响：电脑唤醒后 Claude 额度可能保留睡眠前数据数小时；用户无法判断当前 5 小时、7 天和额外用量。Codex 由于本地 Session/Hook 更容易产生新事件，看起来会先恢复更新。
- 触发：保持 ActRealm/Runtime 运行，让 Mac 睡眠；唤醒后继续使用 Claude，但不重启 ActRealm/Runtime。
- 实际：用户现场多次观察到 Claude 额度长时间不更新；执行诊断、重新连接或重启后又突然恢复。本轮为避免丢失远程测试控制，没有再次强制物理睡眠，因此症状由用户现场反馈，机制缺口由源码确认。
- 机制分析：产品代码没有订阅 `NSWorkspace.didWakeNotification`/睡眠通知；客户端 WebSocket 在 `receive()` 上没有 heartbeat deadline，只有收到错误才重连。Claude OAuth 刷新由 `snapshot_value -> quota_entries` 懒触发；连接在唤醒后如果仍显示 `live` 但不再收到数据，就缺少强制 reconnect/invalidate/refresh 路径。这也解释了为什么“检查一下或重启”可能顺手触发恢复。
- 预期：唤醒后立刻验证连接；强制刷新一次 snapshot 和 Claude OAuth；失败时保留旧值但明确标记“刷新失败/最后成功时间”，并按退避策略重试。
- 证据：源码中不存在睡眠/唤醒监听、WebSocket 无接收 deadline、额度刷新依赖 snapshot，见 [45-sleep-quota-and-lifecycle-source.log](evidence/2026-07-22/45-sleep-quota-and-lifecycle-source.log)。用户唤醒并恢复后的界面证据见 [42-claude-open-creates-history-cards.png](evidence/2026-07-22/42-claude-open-creates-history-cards.png)。
- 准备改进：App 监听 sleep/wake；wake 后取消旧 WebSocket、执行轻量 health request 并重连；Runtime 增加独立 quota scheduler，不依赖是否有 WebSocket 客户端；提供显式 refresh/invalidate API；记录最后尝试、最后成功、失败原因。
- 复测门槛：睡眠 1、10、60 分钟各 5 次；唤醒后 10 秒内连接恢复，60 秒内额度成功刷新或显示明确失败；不重启 App/Runtime也能恢复。

### AR-024 · P1 · 只打开 Claude 就把历史恢复会话列成 Tasks 空卡片

- 层级：Runtime 会话可见性/事件语义。
- 影响：用户没有输入任何新任务，Tasks 却出现多个“apple / AI-Display、模型未知、空闲、刚刚”的卡片；真正运行中的任务被噪音挤占，任务数量失真。
- 触发：打开 Claude Desktop 的 Code 页面，不在任何会话中发送消息。
- 实际：17:19–17:21 期间 Runtime 记录了 10 个 `session.started` 和 11 个 `session.ended`，没有 `prompt.submitted`、工具、审批或问题事件；17:21 的三张空卡分别只存在 1–3 秒，正好对应截图中的三张空闲卡。
- 根因：Runtime 把 SessionStart/SessionEnd 写入 `last_event_at`；服务端可见性规则保留“30 分钟内任何事件”的 session，没有区分“生命周期枚举/恢复”和“真实任务活动”。因此 Claude 自己恢复历史窗口就会刷新最近时间。
- 预期：SessionStart 可以建内部 session，但在用户提交 prompt、运行工具、产生审批/问题/完成等真实任务事件前不得进入 Agent Tasks；已有 attention 的 session 始终可见。
- 证据：[42-claude-open-creates-history-cards.png](evidence/2026-07-22/42-claude-open-creates-history-cards.png)、[43-claude-open-without-new-code-task.png](evidence/2026-07-22/43-claude-open-without-new-code-task.png)、事件聚合 [44-claude-open-event-analysis.log](evidence/2026-07-22/44-claude-open-event-analysis.log)、源码规则 [45-sleep-quota-and-lifecycle-source.log](evidence/2026-07-22/45-sleep-quota-and-lifecycle-source.log)。
- 准备改进：新增 `last_meaningful_activity_at`/`has_meaningful_activity`；只有 PromptSubmitted、工具、审批、问题、Elicitation、真实完成/失败更新该字段；任务列表使用 meaningful 时间的 30 分钟窗口。SessionStart/SessionEnd 只用于内部生命周期和标题映射，不更新可见任务时间。
- 复测门槛：打开含 0、5、50 个历史会话的 Claude，Tasks 均不新增卡；发送第一条 prompt 后只出现对应一个会话；历史会话真正恢复执行或出现 attention 时立即出现。

## 4. 已确认通过的能力

### 4.1 核心、协议和安全

- Rust/Swift 全自动化门禁通过，详见 [01-automated-gates.log](evidence/2026-07-22/01-automated-gates.log)。
- Runtime 仅监听 `127.0.0.1`，不存在对局域网或公网的监听。
- `bridge.sock`、`codex-app-server.sock`、锁文件、数据库、WAL、缓存、备份均只允许当前用户访问，详见 [30-live-network-and-permissions.log](evidence/2026-07-22/30-live-network-and-permissions.log)。
- 导出 JSON 有效，测试命令未明文出现，包含脱敏标记，没有 OAuth、Authorization 或 bootstrap token 字段，详见 [32-live-export-privacy-summary.log](evidence/2026-07-22/32-live-export-privacy-summary.log)。
- Runtime 离线时阻塞 Hook 能静默成功返回，不会把 Provider 永久卡死，详见 [13-runtime-kill-recovery.log](evidence/2026-07-22/13-runtime-kill-recovery.log)。

### 4.2 Provider 行为

- Codex request-keyed 审批能得到正确 allow directive；匹配 PostToolUse 后移除 waiting 状态。
- Codex 原生 `request_permissions` 被识别为 Provider/终端拥有，只展示“打开应用”，没有伪造 ActRealm 批准按钮，证据 [11-native-approval-observation.png](evidence/2026-07-22/11-native-approval-observation.png)。
- auto-review 与 full-access 都不会创建竞争 waiter；任务完成后仍能产生完成确认，证据 [12-provider-owned-completion-notifications.png](evidence/2026-07-22/12-provider-owned-completion-notifications.png)。
- 最终 `doctor` 全项通过，真实 Claude/Codex 事件均有安装后记录，详见 [40-test-cleanup-and-final-doctor.log](evidence/2026-07-22/40-test-cleanup-and-final-doctor.log)。

### 4.3 首装与压力

- 隔离 HOME/ACTREALM_HOME 下，首屏如实显示未连接；Claude 安装/移除能写入和恢复测试配置；Codex 安装后要求用户在官方 `/hooks` 中信任，没有绕过官方步骤。
- 500 个唯一 Codex 会话、16 并发在 2.32 秒内写入；数据库为 500 session、500 event、0 open attention，`integrity_check=ok`；Runtime RSS 5888 KiB。有效结果见 [29-500-session-burst-valid.log](evidence/2026-07-22/29-500-session-burst-valid.log)。
- [28-500-session-burst.log](evidence/2026-07-22/28-500-session-burst.log) 是第一次压力脚本的 shell 引号错误，已明确判为无效并保留审计；不计入产品结果。

## 5. 修复顺序建议

### 第一批：控制正确性与恢复（必须先做）

1. AR-001、AR-002、AR-007：稳定 ID 选择、问题表单、命令状态反馈。
2. AR-003、AR-004、AR-005、AR-006：Runtime watchdog、App 重附着、离线只读、重启状态归约。
3. AR-008：Codex listener 进程归属与版本验证。
4. AR-024：用真实任务活动而不是生命周期噪音决定 Tasks 可见性。

完成标准：并发待办不选错；App/Runtime 任意单方崩溃后 5 秒内恢复；不存在幽灵等待和孤儿进程。

### 第二批：性能、隐私和可访问性

1. AR-009、AR-010：暂停不可见动画、增量 snapshot、活跃负载资源门禁。
2. AR-023：睡眠/唤醒重连与独立额度刷新调度。
3. AR-015：bootstrap 和统一秘密脱敏。
4. AR-016：键盘、VoiceOver、AX 修复。
5. AR-011、AR-012、AR-013、AR-014：语义和持久化收敛。

完成标准：后台/最小化 CPU 达标；秘密不进入任何诊断面；无障碍核心流程可独立完成。

### 第三批：发布工程和产品范围

1. AR-017：确认并固定 HUD/Agent Focus 产品范围。
2. AR-018、AR-019、AR-020：签名、公证、安装器、CI、开机启动、更新、回滚。
3. 冻结新候选，执行 AR-021、AR-022 的长期与兼容性矩阵。

完成标准：干净 Mac 上不绕过安全检查即可安装；所有远端强制门禁通过；48 小时冻结候选浸泡完成。

## 6. 上线判定

### 当前判定

- 开发内部验证：**可继续**，但应避免把 ActRealm 作为唯一审批通道。
- 小范围技术用户试用：**有条件**，必须明确 Runtime 崩溃需手动重启、并发 Outbox 有风险。
- 普通用户公开发布：**NO-GO**。

### 重新申请上线验收的最低条件

1. AR-001 至 AR-010、AR-015、AR-018、AR-019、AR-023、AR-024 全部关闭。
2. Provider 真实验收覆盖 Claude/Codex 的审批、提问、完成、自动审批、完全访问。
3. App/Runtime 故障注入连续 20 次无失败。
4. 整个进程树资源门禁通过，且不存在孤儿 app-server。
5. 签名、公证、干净安装通过。
6. 48 小时 exact candidate 浸泡完成。
7. 用户在本地完成最终视觉和真实工作流验收后，才允许 commit/push/release。

## 7. 证据说明

全部证据保存在 [evidence/2026-07-22](evidence/2026-07-22/)；索引见 [00-evidence-index.txt](evidence/2026-07-22/00-evidence-index.txt)。证据包含日志、SQL 聚合结果、源码锚点、原生界面截图、崩溃报告和压力脚本。测试生成的真实导出 JSON 在检查后已删除，没有把本机完整会话内容写入仓库。

本轮没有修改产品代码、没有 commit、没有 push。测试结束后隔离压力 Runtime 已停止，主 Runtime 的 `doctor` 再次全项通过。
