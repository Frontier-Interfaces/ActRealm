# ActRealm P0 修复与复验报告

日期：2026-07-22
冻结基线：`50b891a4e7f7b8c07349d5a082cfc9dcfc9e7bd0`
提交状态：本报告随 P0 修复由同一 commit 收录；未执行 push
上游问题来源：[全局上线审计](ACTREALM_GLOBAL_RELEASE_AUDIT_2026-07-22.md)

## 1. 结论

本轮已完成九项 P0 的代码修复或发布流程补齐。与本机代码直接相关的
OUTBOX、Runtime、SQLite reducer、Connector 生命周期和后台渲染缺陷均已落地，
完整本地 Rust/Swift 门禁通过。

但本报告不把“代码已修复”写成“产品已可上线”。以下外部门禁仍然开放：

1. Claude 真实 AskUserQuestion/Elicitation 最终端到端回归；
2. App 正常退出、Force Quit、`SIGKILL` 各 20 次的完整矩阵；
3. GitHub 新 CI 的首次远端运行及分支保护；
4. Developer ID 签名、公证、staple、Gatekeeper 和全新 Mac 安装；
5. 连续 48 小时冻结候选浸泡。

因此当前状态是“P0 代码修复候选”，不是最终 v1 发布版。

## 2. 本轮门禁结果

| 门禁 | 结果 | 证据摘要 |
| --- | --- | --- |
| Rust format | 通过 | `cargo fmt --all -- --check` |
| Rust Clippy | 通过 | workspace、all targets、`-D warnings` |
| Rust tests | 通过 | 186 个已发现测试；183 通过，3 个显式 manual/resource 测试按设计 ignored |
| Rust release build | 通过 | `cargo build --workspace --release --offline` |
| Swift tests | 通过 | 78 tests / 14 suites |
| 原生问题表单快照 | 通过 | `interactive-question-light.png` 显示 Claude 单选、其他输入、发送和原窗口回答入口 |
| 产品语言 | 通过 | `check-actrealm-language.sh` |
| Plist | 通过 | `plutil -lint` |
| Workflow YAML | 通过 | Ruby YAML 解析两份新增 workflow |
| Diff whitespace | 通过 | `git diff --check` |
| 本地 QA 打包 | 通过 | arm64 App/Helper、deep strict codesign、SHA/build date 元数据 |
| 发布签名防误用 | 通过 | 无 Developer ID 或脏工作树时均在构建前拒绝 |
| 最小化 CPU | 通过 | 6 次采样 `0.1/0.2/0.1/0.1/0.0/0.1%`，p95 约 `0.2%` |
| Runtime 强杀恢复 | 通过 | Supervisor 连续 20/20 次恢复；最终 stdio 包另做 2 次直接强杀，PID 均替换且生产旧 Socket 保持 0 |

## 3. P0 逐项修复

### AR-001 · OUTBOX 排序后选择错位

- 原影响：点击一个事项后，排序变化可能让主卡指向另一条，造成对错误对象操作的认知风险。
- 触发：并发插入、置顶、解决、忽略审批/问题/完成事项。
- 修复：删除数组下标选择，改为稳定 `attention_id`；每次 snapshot 后按 ID
  重新定位，选中项结束时回退到当前最高优先级开放项。
- 代码证据：`AppModel.swift` 的 `OutboxSelection`、`selectedOutboxID`；
  `OutboxSection.swift` 的 `selectedEntry` 和按 ID 过滤队列。
- 复验：`stableSelectionSurvivesPriorityReordering`、
  `resolvedSelectionFallsBackToHighestPriorityOpenItem` 通过；问题快照可稳定选中
  `att-question`，主卡不再停留在 Codex 审批。
- 状态：**代码与本地门禁关闭**。

### AR-002 · Claude 提问/Elicitation 无法在原生 OUTBOX 回答

- 原影响：V1 的“在待处理栏回答 Agent 问题”在原生客户端不可用。
- 触发：Claude `AskUserQuestion` 或 Elicitation 进入 Runtime。
- 修复：主卡识别带 `interaction` 的 question，渲染 1–4 问、单选、多选、
  自由输入、Boolean、数字、秘密字段；Elicitation 提供 accept/decline/cancel；
  Runtime 离线或请求过期时禁止提交。
- 隐私：草稿只存在 SwiftUI `@State`；秘密答案不进入 AppModel、SQLite、日志或导出。
- 代码证据：`OutboxSection.swift:283`、`InteractiveQuestionView.swift`；后端仍使用
  官方 `updatedInput.answers`/Elicitation reply shape。
- 复验：Rust 的 Claude question、Elicitation、秘密导出测试通过；Swift 快照确认表单
  已出现在选中的主卡。
- 状态：**代码关闭；真实 Claude 双向端到端仍是发布门禁**。

### AR-003 · Runtime 崩溃后不会自恢复

- 原影响：一次异常退出即可让事件、额度和控制全部停止。
- 触发：对 App 管理的 Runtime 发送 `SIGKILL`。
- 修复：Supervisor 增加 `desiredRunning`、持久 bootstrap handler、
  `0.5/1/2/4/8s` 有界退避、连续失败熔断、用户主动停止取消重试；新连接前先断开旧 WebSocket。
- 代码证据：`RuntimeSupervisor.swift:329–374`、`RuntimeClient.connect`。
- 复验：连续 20 次强杀均在观测上界 0.75 秒内获得新 PID；没有第二个 Runtime。
- 状态：**关闭**。

### AR-004 · Runtime 离线仍显示旧控制按钮

- 原影响：用户点击看似有效的批准/拒绝/回答，实际回复通道已经不存在。
- 触发：事项仍在客户端缓存时 Runtime 断开。
- 修复：所有 mutate action 同时要求 `bridgeStatus == listening`；离线主卡显示
  “仅供查看”，批准、拒绝、回答、确认、忽略、撤回全部禁用；跳回 Provider 仍可用。
- 代码证据：`AppModel.swift:987–1028`、`OutboxSection.swift:151–164`、
  `InteractiveQuestionView` 的统一 disabled 状态。
- 复验：`disconnectedRuntimeCannotMutateAttention` 通过。
- 状态：**代码与本地门禁关闭**。

### AR-005 · App 异常退出后无法接管仍存活 Runtime

- 原影响：Runtime 健康但新 App 无 bootstrap token，界面永久离线。
- 触发：只 Force Quit App，保留其 Runtime，再打开 App。
- 修复：启动时读取私有 lock owner，只在可执行路径精确匹配当前 bundled helper、
  已安装 helper、开发 release helper，或满足同一 bundle ID 与签名身份的移动 App helper
  时平滑替换；正式包要求相同 Developer ID TeamIdentifier，ad-hoc QA 包还要求 helper
  归当前用户所有；不识别的 PID 绝不杀。替换后使用新 bootstrap/session 建立客户端连接。
- 代码证据：`RuntimeSupervisor.replaceAbandonedRuntimeIfNeeded` 和
  `isExpectedRuntimePath`。
- 实机证据：一次 Force Quit 后旧 Runtime PID `63460` 被新 PID `63898` 替换；跨临时
  安装路径升级时旧 Runtime PID `69959` 被最终候选的新 PID `11150` 安全替换，任务和额度
  恢复；一次正常退出/重开也恢复。
- 复验：新增 `movedRuntimeHelperRequiresTheSameSignedOrLocallyOwnedAppIdentity`，覆盖同团队
  接受、错误团队拒绝、同用户 ad-hoc 接受、错误 UID 拒绝、bundle ID 冒充拒绝。
- 状态：**代码修复；20×3 完整退出矩阵仍待发布复验**。

### AR-006 · Attention 结束但 Session 仍显示等待

- 原影响：OUTBOX 已空，任务卡和重启恢复却仍显示“等待你批准/回答”。
- 触发：ack/dismiss、超时、stale commit、Runtime restart reconciliation。
- 修复：新增同事务 `release_session_if_unblocked`；只有不存在其他开放 blocker 时，
  清除 widget ownership，把会话转为 `waiting_for_event` 并写入真实原因；旧 waiter
  永不恢复。
- 代码证据：`storage.rs:2540/2657/2736/2798/2808`。
- 复验：Runtime 重启、server Elicitation、秘密导出和现有 35 项 reducer 测试通过。
- 状态：**关闭**。

### AR-008 · Codex app-server listener 形成孤儿进程

- 原影响：多次 Runtime 崩溃后积累 PPID-1 listener、Socket、RSS 和 FD。
- 触发：旧实现启动 `app-server --listen` 后再启动 proxy；Runtime `SIGKILL` 无法运行 Drop。
- 修复：改用 Codex 官方 `app-server --stdio` 直接子进程，取消 listener/proxy/Socket
  中间层；Runtime 消失时 stdin 自动 EOF；Connector Drop 同步 kill/wait 子进程；reader
  与 server handler 使用 Weak，消除 Arc/thread 生命周期环。
- 升级迁移：启动前只扫描绑定 ActRealm 私有旧 Socket 路径、且 command 为 `codex` 的
  listener，发送 TERM；真实 Socket 文件可删除，符号链接或其他文件不触碰。
- 代码证据：`codex-connector/src/lib.rs:105–121`、`:339–388`。
- 复验：stdio 初始化/list/resume/request/response 全链路测试通过；Drop 后 child PID 已被
  reap；遗留 listener 解析只接受精确私有路径。最终候选启动后，生产路径
  `~/.actrealm/run/codex-app-server.sock` 的 listener 数量为 `0`，Runtime 强杀恢复后仍为
  `0`，Bridge 由 lock 中的新 Runtime PID 唯一持有。
- 补充说明：系统中仍可见 5 个修复前测试留下的 `/tmp/fa-m1a-provider-resolution-*`
  listener。它们不是生产路径，新 Connector/新测试不会再创建；产品不会扫描并杀死任意
  `/tmp` 进程，以免越权终止其他软件。重启系统或测试维护可清除这些历史夹具。
- 状态：**代码与生产迁移实机复验关闭**。

### AR-009 · Agent Focus 最小化后持续高 CPU

- 原影响：后台发热、耗电，最小化仍保持约 18.8%–23.7% CPU。
- 触发：打开 Agent Focus 后最小化、遮挡或切换 App。
- 修复：AppKit 观察 App active、窗口 minimized/occluded/visible；Timeline 从 30 FPS
  调整为 15 FPS 并在不可见时暂停；后台 token/session-only snapshot 只缓存最新值，
  不发布整棵 SwiftUI；Attention/command 仍立即投影，恢复可见时一次补齐最新 snapshot。
- 代码证据：`WindowActivityProbe.swift`、`MainWindowView.swift:89`、
  `ForegroundSchedulingView.swift:943`、`AppModel.receive(snapshot:)`。
- 实机复验：最小化 6 次采样为 `0.1/0.2/0.1/0.1/0.0/0.1%`；恢复窗口后 token
  从 `914.4M` 补齐到 `915.1M`，证明暂停的是渲染，不是数据采集。
- 状态：**关闭，达到 p95 < 0.5% 门槛**。

### AR-018 · 发布包不满足普通用户安装标准

- 原影响：ad-hoc 包被 Gatekeeper 拒绝，版本无法追溯，支持范围不明确。
- 修复：package script 写入 commit/build date/build number，验证架构和 deep strict
  signature；发布模式拒绝 ad-hoc 和脏工作树；新增 Developer ID、DMG、公证、staple、
  Gatekeeper、SHA-256 与 GitHub Release workflow。
- 支持矩阵：当前明确为 **macOS 26+ / Apple Silicon**，不再暗示 Intel 可用。
- 本地复验：arm64 QA App/Helper、Plist、deep strict codesign 通过；无身份和脏树保护通过。
- 状态：**发布代码补齐；正式证书、公证服务与全新 Mac Gatekeeper 仍是外部门禁**。

### AR-019 · GitHub 缺少核心 CI 门禁

- 原影响：PR 可在没有 Rust/Swift 构建测试和安全检查的情况下合并。
- 修复：新增 `ci.yml`，覆盖 SDK、Rust 1.85、format、Clippy、workspace tests、release
  build、Swift tests、RustSec、语言、Plist、本地 QA 包和 SHA 元数据；新增独立签名发布 workflow。
- 本地复验：两份 YAML 可解析；与 workflow 对应的本地 format/Clippy/test/release/
  Swift/language/Plist/package 门禁全部通过。
- 状态：**workflow 代码完成；首次远端运行和 branch protection 仍需 push 后验证**。

## 4. 未关闭的发布风险

| 风险 | 为什么不能在本地伪造通过 | 下一步 |
| --- | --- | --- |
| 真实 Claude 问题/Elicitation | 必须由真实 Provider 产生和接收官方回复 | 解锁后启动最终候选，逐类制造并回答 |
| App 退出矩阵 | 当前只有强杀 Runtime 20 次、App Force Quit/正常退出各一次 | 对冻结包执行正常/Force Quit/`SIGKILL` 各 20 次 |
| CI | workflow 尚未 push，GitHub 尚未执行 | 用户验收并授权 commit/push 后观察 required checks |
| 签名与公证 | 本机没有正式 Developer ID/公证 Secrets | 配置仓库 Secrets，发布测试 tag，在全新 Mac 安装 |
| 48 小时稳定性 | 长时门禁不能由短测替代 | 冻结最终 SHA 后再跑，期间不换二进制 |

## 5. 提交边界

本轮 P0 代码、全局审计、复验报告和已检查的证据目录由同一 commit 收录；未执行 push。
`docs/prototypes/actrealm-agent-source-preview.html` 仍是用户明确要求不提交的想法稿，
继续排除在提交范围之外；所有 `.DS_Store` 和本机无关产物也不纳入提交。
