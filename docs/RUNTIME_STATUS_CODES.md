# Runtime status-message contract

Runtime-generated presentation state is represented by a stable message code
and optional parameters. Clients own the final wording and may support any
locale without changing Runtime state.

The machine-readable source of truth is
[`shared/contracts/runtime-messages.json`](../shared/contracts/runtime-messages.json).
The English and Simplified Chinese strings in that file are reference
translations. A client may refine typography or grammar for its platform, but
must preserve the documented meaning and parameters.

API failures follow the same ownership rule. Runtime HTTP responses expose a
stable uppercase `error.code`; clients render it using
[`shared/contracts/api-errors.json`](../shared/contracts/api-errors.json).
The optional Runtime `detail` is diagnostic context and must not be used as
the primary user-facing sentence.

## Payload shape

```json
{
  "code": "session.activity.tool_running",
  "args": {
    "tool": "Bash"
  }
}
```

New clients should render the structured `...Message` field first and use the
legacy English text field only as a compatibility fallback. Provider- and
user-authored text is not a Runtime status message and must remain verbatim.

Current snapshot fields are:

| Runtime object | Structured field | Legacy fallback |
| --- | --- | --- |
| Session | `activityMessage` | `activity` |
| Session / jump response | `jumpMessage` / `labelMessage` | `jumpLabel` / `label` |
| Attention | `titleMessage` | `title` |
| Attention | `detailMessage` | `detail` |
| Attention | `riskMessages` | `riskNotes` |
| Interactive prompt | `titleCode` | `title` |
| Quota | `windowMessage` | `limitName`, `windowMinutes`, `window` |
| Quota | `reasonMessage` | `reason` |

## Core bilingual reference

| Code | English | 简体中文 |
| --- | --- | --- |
| `session.activity.idle` | Waiting for a new task | 等待新任务 |
| `session.activity.thinking` | Thinking | 正在思考 |
| `session.activity.tool_running` | Running `{tool}` | 正在运行 `{tool}` |
| `session.activity.awaiting_approval` | Waiting for your approval | 等待你批准 |
| `session.activity.awaiting_answer` | Waiting for your answer | 等待你回答 |
| `session.activity.compacting` | Compacting context | 正在压缩记忆 |
| `session.activity.completed` | Turn completed | 本轮已完成 |
| `session.activity.failed` | Run failed | 运行失败 |
| `attention.approval.title` | Approval required | 等待批准 |
| `attention.native_approval.title` | Approve in `{provider}` | 请在 `{provider}` 中批准 |
| `attention.question.title` | `{provider}` is asking a question | `{provider}` 正在询问 |
| `attention.error.title` | Agent run failed | Agent 运行失败 |
| `attention.completion.title` | Task completed; waiting for confirmation | 任务已完成，等待确认 |
| `attention.approval.detail` | Review the operation in the original conversation | 请在原对话中核对操作内容 |
| `attention.risk.high_impact` | High-impact operation detected | 已识别到高影响操作 |
| `jump.exact_conversation` | Open exact conversation | 精确打开对话 |
| `jump.terminal` | Open terminal | 打开对应终端 |
| `jump.app_only` | Open application | 只能打开应用 |
| `jump.unsupported` | Jump is not supported | 当前环境不支持跳转 |
| `quota.window.hours` | `{count}` hours | `{count}` 小时 |
| `quota.window.days` | `{count}` days | `{count}` 天 |
| `quota.window.extra_usage` | Extra usage | 额外用量 |
| `quota.reason.cache_missing` | Quota cache is missing | 额度缓存不存在 |
| `quota.reason.no_valid_window` | No verifiable quota window was found | 没有找到可验证的额度窗口 |
| `quota.reason.claude_refresh_failed` | Claude quota refresh failed; showing the last capture | Claude 额度刷新失败，显示上次数据 |
| `quota.reason.codex_refresh_failed` | Codex quota refresh failed; showing the last capture | Codex 额度刷新失败，显示上次数据 |

The JSON contract contains the complete list, including parameters and
long-form quota failure guidance.

## Ownership rules

- Runtime owns codes, parameters, Provider facts, persistence, and transport.
- macOS, Web, and Windows own localized wording and layout.
- Runtime logs and legacy fallback strings use English.
- Raw Provider text, user text, paths, project names, tool names, and model
  names are passed through without translation.
- Adding or removing a code requires updating the shared contract and every
  implemented client in the same change.
- macOS and Web must map every registered API error code. Unknown future codes
  use a localized generic failure message while retaining the code for
  diagnostics.
