# ActRealm R1 Current-Turn Workflow 验证报告

日期：2026-08-17

分支：`agent/runtime-v2-agent-observability`

候选提交：`3df34a5b60df503db0175b6761ac541cee18477b`

安装包：ActRealm `0.1.0` build `35`，Apple Development 签名，arm64

## 结果

R1 的 current-turn 计划、规范化活动、并行工具身份与向后分页已通过自动化和本机真实
Codex 任务验证。Display 仓库和已安装的 Display 应用未在本轮修改。

## 自动化证据

- `cargo test --workspace --no-fail-fast`：通过；显式 release-candidate soak 与手动预览测试
  按既有规则保持 ignored。
- `apps/macos/Scripts/test.sh`：176 tests / 26 suites 通过。
- `plutil -lint apps/macos/Resources/Info.plist`：通过。
- 既有 5,000 session 快照、hook p95、timeline/WebSocket 和隐私回归继续通过。
- 新增回归覆盖 Provider 调用 ID/来源版本、旧数据库增量迁移、规范化 phase/status/
  confidence、共享投影隐藏调用 ID、向后游标、并行同名工具反序完成和能力驱动空态。

## 安装与运行证据

- `/Applications/ActRealm.app` 已替换为 build 35；包内 `ActRealmGitCommit` 与候选提交一致。
- 旧 build 34 被移动到可恢复的
  `~/.Trash/ActRealm-build34-before-r1.app`。
- `/api/v1/health` 返回 `protocolVersion: 2`。
- 启动约 14 秒时进程采样：macOS App RSS 约 100.5 MiB，Runtime RSS 约 36.9 MiB。
  这是一次现场样本，不替代 R6 长时 soak。

## 真实当前任务

验证任务：本次 ActRealm 修改会话。

- 主页面只保留当前运行任务；已结束且无当前待处理事项的历史任务按规则退出列表。
- 任务流程显示 6 个真实结构化步骤：4 completed、1 in progress、1 pending，与 Codex
  当前计划一致。
- 工作流首屏显示最近 54 个折叠条目；点击“加载更早事件”后显示 88 个条目，视图仍停在
  实时尾部，当前执行项保持可见。
- 本机 SQLite 的新事件已带 `tool_call_id`；抽查 Bash start/end 使用同一
  `exec-*` 身份，证明新版本不再只按工具名配对。
- Codex 未提供 Hook/source version 时 `source_version` 保持空值，没有猜测版本。
- 用户的 custom 显示设置原本关闭 `workflow`；验证时只启用该安全字段，其他自定义字段
  未重置。

## 已知边界

- 当前 Turn 在 build 34 运行期间已经开始，因此升级前的遗留事件没有 `tool_call_id`；
  跨升级边界的少量条目会诚实显示“未收到结束事件”，不会冒险错配。下一个完整 Turn
  从第一条事件起即可使用精确身份。
- Provider 报告工具名为 `Unknown` 时 UI 显示中性“工具”，不会从命令或调用 ID 猜测
  具体工具。Provider 明确给出 `Bash` 等名称时会显示真实名称与安全语义分类。
- Claude Code 本机 OAuth 仍是先前记录的过期状态，本轮没有伪造 Claude 成功链路；
  Claude fixture、能力矩阵和 Swift/Rust 回归已经通过，真实成功任务留给凭据恢复后的
  R6 soak。
