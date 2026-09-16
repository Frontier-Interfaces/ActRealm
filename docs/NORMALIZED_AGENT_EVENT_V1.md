# Normalized Agent Event v1

日期：2026-08-17

状态：ActRealm Runtime 与 macOS 本地工作流已实现；该文档冻结 v1 的语义和隐私边界。

## 目的

Claude Code 与 Codex 的 Hook 名称和字段并不相同。ActRealm 必须先把它们映射为一组
稳定事实，再让任务流程、工作流和后续 Display 消费；UI 不应直接猜测 Provider 原始
事件的含义。

## 本地 Timeline 事件

每条 `/api/v1/sessions/{sessionId}/timeline` 事件包含：

| 字段 | 语义 |
| --- | --- |
| `schemaVersion` | 固定为 `1`；未知版本必须按未知能力降级 |
| `eventId` | Runtime 本地事件身份 |
| `provider` | `claude` 或 `codex` |
| `turnId` | Runtime 已解析的当前 Turn 身份；可能为空 |
| `phase` | `session`、`turn`、`tool`、`attention`、`plan`、`subagent` |
| `kind` | 稳定的事实类型，例如 `tool.started`、`plan.updated` |
| `status` | `started`、`running`、`completed`、`failed`、`interrupted`、`requested`、`resolved` 或 `updated` |
| `occurredAt` | Provider 事件发生时间（毫秒） |
| `ingestSequence` | Runtime 单调写入序号，用于稳定分页和增量读取 |
| `toolName` | 经过长度和控制字符约束的真实工具名；未知时为空 |
| `toolCategory` | Runtime 的安全语义分类，不替代真实工具名 |
| `toolTarget` | 仅本机认证 UI 可见的、来自允许字段的 basename；不解析 shell 命令猜测 |
| `toolCallId` | 仅本机认证 UI 可见的 Provider 调用身份，用于正确合并并行同名工具 |
| `sourceVersion` | Provider 明确提供的受限 Hook/schema 版本；未提供时为空 |
| `confidence` | `provider_fact` 或 `runtime_derived` |

Timeline 绝不返回 prompt、完整路径、命令、工具输入/输出、文件内容或 transcript。Team
和 Companion 的共享投影不返回原始 `toolCallId` 或 `toolTarget`。

## Provider 能力

`shared/contracts/provider-capabilities.json` 是唯一能力声明。v1 明确声明 `plan`、
`subagents`、`approvals`、`transcriptSlice`、`toolLifecycle` 和 `currentTarget`。缺失
Provider、缺失字段、未来状态或无来源的 `supported` 一律降级为 `unknown`，UI 不得把
可见状态反推为能力。

当前事实来源：

- Claude Code 工具生命周期：`PreToolUse`、`PostToolUse`、`PostToolUseFailure`。
- Codex 工具生命周期：相同 Hook 事实；结构化计划优先使用 connector 的
  `turn/plan/updated`，observe-only 会话可使用 allowlisted `update_plan` Hook。
- Claude 计划：`TaskCreated`、`TaskCompleted`。
- current target：仅 `tool_input` 中允许的路径字段，最终只保留 basename。

## Current Turn 规则

- 新 prompt 创建或选择新 Turn 时，清空上一个 Turn 的当前计划和 Task 投影。
- Codex 延迟到达且不属于最新 Provider Turn 的计划不能覆盖当前计划。
- 当前工作流查询只返回最新 Turn 的事件；旧 Turn 不得冒充仍在执行。
- Provider 没给 Turn ID 时，Runtime 只能关联当时的当前 Turn，不能创建永久的 Provider
  假 ID。
- 计划未知时不显示虚假百分比；没有结构化计划与 Provider 不支持计划是不同空态。

## 工具生命周期合并

- 有 `toolCallId` 时必须按 `toolCallId` 和 Turn 配对 start/end；不能只按工具名。
- 没有调用 ID 时才允许按同 Turn、同工具名保守回退。
- 一对 start/end 在 UI 中只生成一行，显示真实工具名、允许的 target、结果和耗时。
- 相邻、短时、成功的常规 Bash 可折叠；运行中、失败、超过 10 秒或需注意的条目保持
  独立。
- 没收到结束事件时明确显示“未收到结束事件”，不伪造成功。

## 分页

- 首屏使用 `latest=true&currentTurn=true` 读取最新 100 条并按时间正序返回。
- `beforeIngestSequence` 读取同一当前 Turn 的更早一页；返回结果仍为正序，便于 UI 前插
  并保持滚动锚点。
- `afterIngestSequence` 用于正向增量；同一请求不得同时提供 before 与 after。
- macOS UI 最多保留 1000 条当前 Turn 事件，达到上限会明确提示，避免长任务无界占用
  内存。

## 降级规则

- Provider 不支持：明确显示“不支持”。
- 能力尚未验证：显示“能力尚未确认”。
- 支持但运行中未收到事件：显示“等待当前 Turn 的首个事件”。
- 支持且 Turn 已结束但无调用：显示“当前 Turn 没有工具调用”。
- 读取失败：保留已经加载的工作流，只在局部显示错误和重试入口。
