# ActRealm H7 分层诊断与恢复

日期：2026-08-25

分支：`actrealm最新版`

状态：实现、自动化门和精确候选真实 UI 核对通过；等待用户最终验收。H8 不能因为本报告提前
启动。

精确候选：`5ddcfb0fbd24364c3e3a84163eaf0e195d004056`，ActRealm `0.1.0 (81)`，
Apple Development 签名，arm64。

## 目标

H7 不再增加日志数量，而是回答用户实际遇到的三个问题：

1. 为什么显示“未连接”；
2. 哪一层出了问题；
3. 可以只恢复这一层，还是必须回到 Provider。

## Runtime 诊断合同

`GET /api/v1/runtime/status` 继续要求本机认证，schema 升至 v2。冻结合同位于
`shared/contracts/runtime-diagnostics.schema.json`，包含：

- Runtime version、构建 commit、Companion protocol、instance、PID、启动时间和 uptime；
- API、WebSocket、Hook socket 权限与最后真实事件；
- Snapshot revision 来源、最后事件和 live/delayed/stale/invalid/unavailable；
- SQLite event count、当前/预期 schema 和 `PRAGMA quick_check(1)`；
- Review baseline collector、pending 数、失败数和 Git `on_demand` 边界；
- Token collector 的扫描状态、数据质量、历史完整性、失败数和最近成功时间；
- Companion protocol、配对数和三个闭合 scope；
- H6 mobile remote approval 与 Claude Cowork 的 neutral conditional state。

不允许：Prompt、完整命令、路径、文件内容、Diff、Token 值、凭据、Transcript、Runtime secret、
Provider reply channel 或任意 Provider 原始 payload。

Runtime commit 只有在编译环境提供合法 40 位 hex 时才返回，否则明确为 `unknown`。打包脚本会把
当前 Git commit 注入 Rust helper；非法或任意环境文本不能进入诊断响应。

## Collector 与存储真实性

- SQLite diagnostics 由唯一 Runtime storage writer 执行；首次 `quick_check` 后缓存 60 秒，避免
  打开面板时反复阻塞写入；
- Review thread 显式报告 scanning/ready/degraded、连续失败和最近成功；非 Git Task 不会把
  collector 本身标成失败；
- Git 状态仍只在用户打开对应 Task Review 时按需读取，诊断页不周期执行 Git；
- Token 状态复用 canonical session ledger 的既有 collection quality，不建立第二套推算；
- Snapshot revision 明确标注 `runtime:sqlite_event_count`，不伪造百分比或剩余时间；
- 有活跃任务时 stale/invalid 才构成投影故障；没有新任务时旧事件时间本身不是故障。

## macOS 交互

原 Runtime Monitor 保留入口，默认内容改为：

1. Control Plane Facts：Runtime、协议/实例、Snapshot、SQLite；
2. Diagnostic Layers：Runtime/Hook、Provider/Connector、Review/Git、Token、Companion、投影/UI；
3. Provider Surfaces：Claude Code、Codex、Claude Cowork；
4. 条件层说明：H6 Cloud/Push/mobile 和 Cowork 不计为当前 H7 故障；
5. 技术详情：进程、锁、socket、endpoint 和最近 Runtime 输出默认折叠。

每层恢复只作用于本层：

- Runtime 不在线时才提供受控 Runtime restart；
- Provider 层重新检查 setup、Doctor version 和 capability；
- Review、Token、投影只刷新 Snapshot/诊断，不修改 Hook 或 Provider 配置；
- Companion 配对/撤销继续留在设置，不在诊断页创建隐式连接；
- Cowork 无已验证任务生命周期来源，明确显示 unsupported，不伪装成 Claude Code。

面板使用 640–900 pt 自适应宽度、560–880 pt 实际窗口高度和滚动内容；Snapshot QA 使用更高的
确定性画布，不改变实际窗口约束。中英文、浅色/深色与技术文本截断均沿用现有设计系统。

## 自动验证

- `cargo fmt --all -- --check`：通过；
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过；
- `cargo test --workspace --offline`：通过；
- `cargo build --workspace --release --offline`：通过；
- Runtime storage data tests：9/9，新增 schema/integrity/path exclusion；
- Server API：8/8，新增 schema v2、collector、conditional、隐私字段缺失；
- macOS ShareProjection：35/35；
- Swift Testing：194/194，新增 H7 response 与 Doctor version 解码；
- language/runtime contract、Info.plist、entitlements、JSON schema、`git diff --check`：通过；
- UI deterministic snapshot 已渲染并进行视觉检查。

## 精确候选实机证据

- build 81 已安装到 `/Applications/ActRealm.app`；原 build 80 移到废纸篓
  `ActRealm-build80-pre-H7.app`，可恢复；
- App Info 与 Runtime 均显示精确 commit `5ddcfb0...`；
- `codesign --verify --deep --strict` 通过；
- Doctor `overall=pass`、`runtime.control_loop=pass`；
- Claude Code `2.1.226` 与 Codex CLI `0.144.6` 版本检查通过；
- Claude/Codex 安装后真实事件均为 pass；
- 实机诊断页显示 API v2、Companion v2、SQLite schema 34、integrity ok；
- Snapshot revision 26,551+ 且 live，Runtime PID 与 instance 可见；
- Review collector ready，Git 明确为 Task Review 打开时 `on_demand`；
- Companion 有 1 个客户端，scope 为 `snapshot.read`、`session.jump`、
  `attention.respond`；
- Claude 能力合同 6/6，Codex 4/6 且 Connector connected；
- Claude Cowork 显示 unsupported/no verified event source；
- H6 mobile/Cloud/Push 显示暂停且不计为 H7 故障；
- Token 首次扫描真实显示 scanning/rebuilding/未完成，仅 Token 层为黄色；Runtime、Provider、
  Review、Companion 和 projection 仍为绿色，没有跨层误报；
- 面板可滚动到底，Provider 与条件层可读；技术详情默认折叠，可展开并显示 endpoint、Helper、
  lock owner 和最近输出；两条 bootstrap token 均为 `<redacted>`；
- Provider “刷新接入状态”只重新读取 setup/Doctor/capability，不重启 Runtime 或改写 Hook。

## 仍需用户验收

- 用户逐项确认 Review、事实来源、历史、Checkpoint、Token、Provider 边界和“为什么未连接”；
- 用户未确认前 H7 状态保持 Candidate，H8 不启动。
