# ActRealm A2 Agent 看板事实层验收报告

日期：2026-08-17

## 结果

ActRealm build 39 已完成 A2 并安装到 `/Applications/ActRealm.app`。本轮没有修改 Display。
Runtime SQLite schema 仍为 28，原有 177 个会话和事件历史在升级后保留。

## 已实现

- 任务名使用 Provider 正式标题优先；第二行明确标为安全“任务摘要”，项目缺失显示
  “项目未知”，不会把摘要伪装成完整 prompt。
- 当前动作同时显示语义类别和安全工具名；`mcp__node_repl__js` 显示为
  `代码执行 · MCP node_repl.js`。分页投影再次脱敏时保持幂等，不退化成匿名“工具”。
- 当前文件只接受 Provider 明确 path 字段并保存 basename；没有事实时按 Provider 能力和
  Turn 状态显示诚实空态。
- 当前 Turn 计划在新 prompt 时清理旧计划；工具 start/update/end 按 invocation identity
  合并。计划和工作流独立滚动，工作流支持向前分页。
- 展开层显示会话/Turn Token、输入/输出/cache/reasoning、上下文，以及 `usageSource` 和
  `usageQuality`。未知用量不再显示假的 0；无真实 subagent 时显示能力空态而非虚构数量。
- Web 设置升级到 v5，新增“当前文件 / 目标”；非自定义 preset 自动迁移，自定义字段选择
  保持用户原样。
- 额度 reset 增加 `resetSource`、`resetCapturedAt`。OAuth 百分比更新但 reset 为 null 时，
  不会擦除同窗口仍有效的官方 StatusLine reset；过期 reset 不保留。UI 区分官方
  StatusLine、官方 OAuth、官方 Codex、本机预计和 Provider 未提供。

## 真实 UI 证据

### Codex 当前任务

- 主卡显示项目、`gpt-5.6-sol`、安全摘要、任务/阶段时间、上下文和累计 Token。
- 当前动作显示 `代码执行 · MCP node_repl.js`。
- 展开计划显示 4 项完成、1 项进行中、3 项待处理；来源为 Codex 结构化计划事件。
- 工作流分页后精确显示多条 `MCP node_repl.js`、Bash、apply_patch，含语义类别、状态和
  持续时间；真实发现的二次脱敏问题修复后再次用 Computer Use 验证。

### Claude Code smoke

- 会话 `ActRealm A2 Claude smoke` 使用真实 Claude Code 2.1.226、`claude-sonnet-5`；只读取
  `/tmp/actrealm-a2-claude-smoke/README.md` 的一行非敏感 fixture。
- 第一次被 $0.05 测试预算门终止，续接同一会话后成功完成；Runtime 正确呈现失败与完成
  生命周期，没有生成第二个重复任务。
- 主卡显示项目 `actrealm-a2-claude-smoke`、模型、完成待确认、101K Token、上下文 3%。
- 展开层显示 `Claude transcript · 完整派生`、输入 8、输出 250、cache read 83.5K、cache
  creation 17.4K；无计划时显示“当前 Turn 已结束，未提供计划”。
- 工作流显示 `文件查询 · Bash` 和 `文件读取 · Read · README.md`，并显示真实持续时间。
- Claude OAuth 实测返回 5h 4%、7d 7%、Extra Usage 90%，三个 reset 均为 null；UI 正确
  显示“重置时间 · Provider 未提供”，没有推算假的倒计时。

## 自动化与构建

- Rust fmt、Clippy workspace/all-targets、workspace test、release build：通过。
- macOS：179 tests / 26 suites，通过。
- Web：11 项 Node 合同测试通过；`web/app.js` 仍低于 128 KiB 单资源预算。
- Runtime/语言合同、`git diff --check`、签名、arm64 和深度签名验证：通过。
- build 39：Apple Development 签名；Runtime schema 28；本机前端与 bundled Runtime 在线。

## 仍属于 A3/A4 的范围

- 历史 Token 的继承基线、日账本、费用覆盖和 Agent 时间口径仍按 A3 计划修复；A2 只保证
  当前会话事实和完整性标签正确，不把现有历史峰值宣布为可信。
- 睡眠唤醒、7 天 soak、诊断恢复和正式候选发布属于 A4。
