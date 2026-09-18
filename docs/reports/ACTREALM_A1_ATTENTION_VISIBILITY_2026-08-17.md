# ActRealm A1 Attention 与任务可见性报告

日期：2026-08-17

## 结果

ActRealm build 38 已完成 A1 的核心实现并安装到 `/Applications/ActRealm.app`。本轮只修改
ActRealm，没有修改 Display。

## 用户可见行为

- 自动隐藏模式：点击“知道了”只关闭完成提醒和 Outbox 项；任务保持已完成状态，到该任务
  原先确定的 `autoHideAt` 后才隐藏。
- 手动模式：点击“确认完成”后立即隐藏任务。
- 之后修改全局隐藏策略不会倒推修改已经完成任务的 deadline；新设置只作用于未来完成。
- 隐藏后的会话、事件、计划、工作流和 Token 历史仍保留；新的真实活动会让隐藏任务重新
  出现。
- 两个客户端竞争处理同一提醒时只有第一个成功，后续动作返回 stale，不会重复变更。

## Runtime 事实模型

- SQLite schema：28。
- `attention_items` 新增 `reminder_acknowledged_at`、`reminder_resolution`。
- `sessions` 新增 `task_hidden_at`、`task_hidden_reason`。
- 分辨 `reminder_acknowledged`、`ack_hidden`、`auto_hidden`、
  `superseded_by_activity`，不再用一个 resolution 同时表达提醒和任务可见性。
- UI snapshot 排除已隐藏任务，完整 snapshot 继续保留历史。

## 排序与焦点

- 固定优先级：错误/失败 → 可直接处理授权 → Provider 原生等待 → 问题 → 运行 → 完成 →
  空闲。
- 同级等待项最早优先；同级运行任务按 Turn 开始时间保持稳定，普通工具事件不会持续抢位。
- 展开的运行任务只在同级中保持靠前；更高优先级 Attention 仍可打断。
- Web 保持当前 Attention，只有更高优先级新请求才自动切换；已读完成提醒不再占用 Outbox。

## 验证

- Rust fmt、Clippy、workspace test、release build：通过。
- Runtime：43 unit 与 66 项核心集成测试等工作区测试全部通过。
- macOS：179 tests / 26 suites，通过。
- Web/Schema：24 项 Node 合同测试，通过；`web/app.js` 保持 128 KiB 单资源预算内。
- Runtime/语言合同、Info.plist、`git diff --check`：通过。
- build 38：Apple Development 签名，arm64，深度签名校验通过。
- 本机 Runtime：schema 28；新 attention/session 列存在；macOS 前端与 bundled Runtime 正常
  运行并建立本地连接。

完整 workspace test 首次运行有一个 Runtime socket ready 时序项失败；该项立即单独复测
通过，随后完整 workspace test 再次运行并全部通过，因此没有忽略或绕过失败。

## 待用户验收

1. 自动模式完成一个真实任务，点击“知道了”，确认 Outbox 消失、任务保留且到期才隐藏。
2. 手动模式完成一个真实任务，点击“确认完成”，确认任务立即隐藏。
3. 同时运行多个 Codex/Claude 任务，确认列表不会因每个工具事件抖动，并优先展示真正需要
   用户处理的任务。

A1 验收前不进入 Display；验收通过后继续 A2 的 ActRealm Agent 看板事实来源收口。
