# ActRealm Claude 额度重置时间调研与实测计划

日期：2026-08-17

范围：只记录数据来源、实现决策和后续验收；本报告不代表运行代码已经完成修改。

## 1. 本机现状与结论

当前本机已经具备真实 Claude 联调条件：

- Claude Desktop 正在运行，并启动其内嵌 Claude Code `2.1.229`。
- 用户级 Claude 设置存在 ActRealm StatusLine command，并安装了 Session、Prompt、Tool、
  Permission、Subagent、TaskCompleted、Stop 等 Hook。
- ActRealm 当前 Claude quota cache 来源为 `oauth_usage`。本次采样成功返回 5 小时 `2%`、
  7 天 `7%`、Extra Usage `90%`，但三个窗口的 `resetsAt` 都是 `null`。
- 当前 UI 没有重置时间的直接原因，是这次官方 OAuth usage 数据没有返回时间，不是
  Swift/Web 已拿到时间却漏画。

现有 Runtime 已能解析两种官方格式：OAuth usage 的 ISO `resets_at`，以及 Claude
StatusLine `rate_limits.*.resets_at` 的 epoch 秒。但目前 OAuth 刷新会生成整份 cache，
而 StatusLine capture 也会生成整份 cache；这会产生一个风险：后到的 OAuth 百分比若
携带 null reset，可能覆盖先前 StatusLine 提供的有效 reset。后续应改成按窗口、按字段
合并，而不是在 UI 层猜时间。

## 2. 官方能力边界

Claude Code 官方 StatusLine 文档列出了：

- `rate_limits.five_hour.used_percentage`
- `rate_limits.seven_day.used_percentage`
- `rate_limits.five_hour.resets_at`
- `rate_limits.seven_day.resets_at`

reset 是 Unix epoch 秒。这些字段只对 Claude.ai Pro/Max 可用，并且通常要等首个 API 响应
后才出现；字段在初始或不可用状态可以缺失/null，消费者必须有安全空态。官方错误文档
还说明 `/usage` 能查看计划限制与重置时间。因此“StatusLine 官方字段 + OAuth 官方
usage”应当是 ActRealm 的真实来源，“按窗口时长推算”只能是次级且明确标注的估算。

参考：

- [Claude Code StatusLine 官方文档](https://code.claude.com/docs/en/statusline)
- [Claude Code 用量与错误官方文档](https://code.claude.com/docs/en/errors)

## 3. 其他项目怎么处理

| 项目 | 数据源 | reset 缺失时 | 值得采用 | 不直接照搬 |
| --- | --- | --- | --- | --- |
| [claude-code-statusline](https://github.com/xkelxmc/claude-code-statusline) | Claude Code 2.1.80+ 优先原生 StatusLine；旧版本/模型额度用 OAuth | 没有 reset 就不输出 reset 文本 | 原生优先、11 分钟 cache、后台刷新、共享锁和退避 | Shell UI 与 ActRealm 架构不同 |
| [usage-monitor-for-claude](https://github.com/jens-duttke/usage-monitor-for-claude) | OAuth usage | null reset 不显示倒计时；测试明确覆盖该空态 | dynamic/scoped limits、自适应轮询、stale 提示、只在有值时显示 | 不能把 OAuth 当永远完整的数据源 |
| [Claude-Code-Usage-Monitor](https://github.com/Maciek-roboblog/Claude-Code-Usage-Monitor) | 本地 JSONL/session block，可吸收 limit message 的 reset | 没官方值时可用 `start + 5h`，但标为 `local_estimate` | 明确区分官方与估算，官方 limit message 优先 | 本地估算无法覆盖其他设备的消耗，不适合作为默认真相 |

Claude Code 的已知现实边界也要进入测试：多个打开的会话可能持有不同新鲜度的
StatusLine rate limit；在某一会话运行 `/usage` 会刷新该会话的数据。模型级周限额也可能
在 `/status` 可见，但未通过 StatusLine 传给第三方。相关上游问题：

- [多会话 StatusLine rate limit 可能陈旧](https://github.com/anthropics/claude-code/issues/75408)
- [模型级周限额没有进入 StatusLine](https://github.com/anthropics/claude-code/issues/73770)
- [OAuth usage 与本地估算的跨设备差异](https://github.com/Maciek-roboblog/Claude-Code-Usage-Monitor/issues/202)

## 4. ActRealm 数据决策

### 4.1 按字段而不是按文档选来源

每个 `provider + account + limit_id/window` 分别存储：

- `used_pct`、`used_pct_source`、`used_pct_captured_at`
- `resets_at`、`reset_source`、`reset_captured_at`
- `window_minutes`、`label/model scope`
- `stale_reason`、`last_success_at`

例如 OAuth 可以提供更新的 `used_pct`，同时 StatusLine 提供该窗口更新且非空的
`resets_at`。两者可以组成一个窗口，但 UI 必须分别保留来源，不能把合并结果统一标成
“OAuth”。

### 4.2 reset 来源顺序

1. 最新、未过期、同账户同窗口的官方 StatusLine reset。
2. 最新、未过期、同账户同窗口的官方 OAuth reset。
3. 同账户同窗口的上次官方 reset；只保留到 reset 到期，且新响应没有明确否定窗口。
4. 用户明确允许时的本机估算，始终标记“预计/本机推算”。
5. 无任何可靠值时显示“重置时间未提供”。

不要直接取多个会话中最大的 reset；应按采样时间、新鲜度、窗口身份和是否仍在未来选择。
不要跨账户、跨模型 limit 或从 5 小时窗口向 7 天窗口复制时间。Extra Usage 没有 reset
时保持无时间状态。

### 4.3 刷新策略

- 正常 Claude 响应带来的 StatusLine 是低额外成本的首选采样。
- OAuth 使用单进程共享锁、退避和自适应轮询；UI 手动刷新不能制造并发请求风暴。
- 临近已知 reset 时允许一次对齐刷新；应用空闲、离线、睡眠时降低或暂停频率。
- 网络失败时显示上次成功值、数据年龄和失败原因；不能把 stale 值显示成实时值。

## 5. Claude 真实联调矩阵

现在 Claude 已经打开，A2–A4 必须用真实 Desktop 内嵌 Code 任务完成以下验证：

| 范围 | 必须核对的字段/行为 |
| --- | --- |
| 任务身份 | 任务名、项目名、Agent 来源、模型、Runtime 状态，不读取完整 prompt |
| 当前状态 | 当前动作、阶段、工具 start/update/end、当前文件 basename、阶段/任务时长 |
| 计划与工作流 | 当前 Turn 计划、完成后清理、工作流分页/滚动、无数据能力空态 |
| Token/上下文 | 当前 Turn/任务 Token、输入/输出/cache、上下文窗口、来源与完整性 |
| 额度 | 5h、7d、scoped/model、Extra Usage、百分比、reset、source、fresh/stale |
| 交互 | question、permission、过期/重复处理、完成提醒、继续运行、subagent 真实事件 |
| 跳转 | 能恢复确切当前会话时跳转；否则只打开 Claude，不自动打开任意历史会话 |

隐私 allowlist 只保留数值、版本、规范化状态、工具安全名和 basename；不保存/输出
credential、完整 session id、原始 payload、完整路径、prompt、命令参数或回复正文。

## 6. reset 专项自动化与现场验收

自动化 fixture：

1. OAuth 的两个核心窗口都有 reset。
2. OAuth 百分比存在但所有 reset 为 null（当前真实形态）。
3. StatusLine 给出 reset，随后 OAuth null；有效 reset 不被擦除。
4. StatusLine/OAuth reset 冲突；按最新有效同窗口采样选择并记录来源。
5. reset 已过期、跨年、夏令时/时区变化、系统时钟回拨。
6. 账户切换、模型 scoped limit 改名、inactive limit、Extra Usage 无 reset。
7. 多 Claude 会话一新一旧、Runtime 重启、睡眠唤醒、网络失败与限流退避。

现场验收：

1. 在当前 Claude Desktop 真实任务发送至少一条请求，触发首次 StatusLine rate limit 数据。
2. 只抓取 allowlist 后的数值字段，分别记录 StatusLine 和 OAuth 是否提供 reset。
3. 与 Claude `/usage` 或 Desktop 官方用量 UI 人工对照百分比和重置时间。
4. 验证无 reset 时显示“Provider 未提供”，有 reset 时倒计时随时间变化但不过度刷新网络。
5. 同时运行 Desktop 内嵌 Code 与一个独立 CLI 会话，验证陈旧会话不会延长 reset。

## 7. 完成标准

- Claude 百分比与 reset 均可追溯到字段级来源；UI 不暗示 OAuth/StatusLine 永远完整。
- OAuth null 不擦除仍有效的官方 StatusLine reset；过期 reset 不残留。
- 官方值、stale 官方值、本机预计和 unavailable 四种状态在 UI/API/导出中可区分。
- Claude Desktop 与 CLI 的任务、计划、工作流、Token、额度和交互真实链路各有证据。
- 原有 Codex 额度、Token 图表、设置、Agent Focus、任务跳转和 Companion 行为无回归。
