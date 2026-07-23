# ActRealm `50b891a` → P0 分支版本提升报告

日期：2026-07-23

主版本基线：`50b891a4e7f7b8c07349d5a082cfc9dcfc9e7bd0`

P0 对比目标：`1ced062fb89f87acc60a877cf82741b9a847aba0`

对比方式：固定 SHA 对固定 SHA，不依赖容易变化的本地分支名称

报告结论：P0 版本显著增强了 OUTBOX 正确性、Provider 生命周期同步、Runtime 自恢复、后台实时性、额度恢复、隐私保护、CI 和发布门禁；它是比 `50b891a` 更可靠的候选版本，但仍不能直接等同于“已完成全部上线验收”。

## 1. 版本范围

`50b891a` 到 `1ced062` 共包含 4 个提交：

| 提交 | 作用 |
| --- | --- |
| `b8559af` | 第一轮 P0 缺陷修复、审计证据、CI 与 macOS 发布工作流 |
| `5d67ed8` | 将 workspace MSRV 和 CI Rust 工具链统一到 Rust 1.97 |
| `e3a7aa0` | 将核心 CI 与 macOS 发布门禁统一到 macOS 26 |
| `1ced062` | 修正第一轮 P0 的回归，并补齐 Provider 生命周期、额度恢复、后台实时刷新和 M15 Codex 管理审批 |

整体变更量：

- 99 个文件发生变化；
- 总计 `+9,921 / -381` 行；
- 其中审计证据目录约 `+5,161` 行；
- 排除证据文件后，产品代码、测试、工作流和文档约 `+4,760 / -381` 行；
- macOS 端约 `+1,096 / -92` 行；
- Rust Runtime/Provider/Connector 端约 `+2,266 / -235` 行；
- GitHub CI/发布工作流新增约 199 行。

这些数字包含测试和文档，不能简单理解为新增了同等规模的产品功能。

## 2. 用户可感知的整体提升

| 领域 | `50b891a` 的主要问题 | P0 目标版本的提升 |
| --- | --- | --- |
| OUTBOX 选择 | 多事项排序后可能选错对象 | 使用稳定 Attention ID，排序变化不再把按钮指向另一事项 |
| Claude 提问 | 原生 OUTBOX 无完整回答表单 | 支持 AskUserQuestion 与 Elicitation 的真实输入和回复 |
| Codex 原生请求 | 某些审批、插件安装/连接请求不出现，或被过早清除 | 保留 Provider 原生等待状态，补充插件安装/连接事项，直到有权威结束事件 |
| Codex 直接审批 | Connector 只能观察部分等待状态 | 对明确接入 ActRealm 管理连接的 Thread，增加版本门禁下的命令、文件和权限审批 |
| 完成状态 | 原生审批仍在等待时，任务可能提前显示完成 | 阻止假完成；审批权威解除后再补发真实完成 |
| Runtime 崩溃 | Runtime 退出后 App 不会可靠恢复 | 有界退避自动重启，并重新建立 WebSocket 和 Bridge |
| App 重开 | App 退出、旧 Runtime 存活时可能无法重新接管 | 只对身份和路径可信的遗留 Runtime 做安全替换并恢复连接 |
| 后台实时性 | 切换到 Codex/其他 App 后，计时和状态可能停住 | ActRealm 窗口仍可见时继续实时更新；最小化或真正遮挡时才暂停高频渲染 |
| CPU | WebSocket 高频重建完整 SQLite 投影，最小化 Agent Focus 仍可能高 CPU | revision 缓存、可见性调度和低频后台策略显著降低空闲资源占用 |
| Claude 额度 | 睡眠后可能长期不刷新，也没有主动恢复入口 | 增加 wake 重连、Runtime 独立轮询和“立即更新”按钮 |
| 历史卡片 | 仅打开 Claude Desktop 就可能把历史会话重新显示为当前任务 | 生命周期回放不再制造可见任务；只有真实有效活动才显示 |
| 状态文案 | 全访问、自动审批、非交互模式容易混用同一文案 | 分离权限模式和审批所有者，避免把观察状态说成自动审批 |
| 隐私 | bootstrap token 可能进入可见诊断尾部 | bootstrap 和诊断输出统一脱敏 |
| CI | 缺少完整核心门禁，初版工具链配置也不兼容锁定依赖 | 增加 Rust/Swift/安全/打包门禁，并统一 Rust 1.97、macOS 26 |
| 发布 | QA 包缺少完整可追溯和正式发布流程 | 增加构建元数据、架构/签名检查、DMG、公证、staple 和 Gatekeeper 工作流 |

## 3. OUTBOX 与交互正确性

### 3.1 稳定选择，避免操作错对象

基线版本的 OUTBOX 选择依赖排序位置。并发插入、解决、忽略或优先级变化后，界面可能仍保存旧数组下标，使主卡和按钮实际指向另一条事项。

P0 版本改为：

- 以稳定 `attention_id` 作为选择键；
- 每次 snapshot 更新后按 ID 重新定位；
- 当前事项结束时回退到最高优先级的开放事项；
- 新审批或问题可以替换当前显示的低优先级完成事项；
- 新的低优先级事项不会抢走正在处理的审批。

实际价值：降低用户在多 Agent 并发时误批、误拒、误确认另一任务的风险。

### 3.2 Claude AskUserQuestion 与 Elicitation

P0 版本在 macOS 原生 OUTBOX 中补齐：

- 1–4 个问题；
- 单选；
- 多选；
- 自由输入；
- Boolean 与数字字段；
- `isSecret=true` 的秘密输入；
- Elicitation 的 accept、decline、cancel。

安全边界：

- 草稿只保存在 SwiftUI 临时状态；
- 秘密答案不进入 AppModel、SQLite、日志、诊断或导出；
- Runtime 离线、请求过期或回复通道丢失时禁止提交；
- Runtime 重启前的旧 waiter 不会被伪恢复。

这使“在 OUTBOX 回答 Claude 的问题”从后端能力变成了原生 macOS 可用能力。

### 3.3 Provider 原生审批不再被假完成覆盖

Codex Desktop 的某些原生权限窗口会在窗口仍然存在时，先发出同工具的 `PostToolUse`、`PostToolUseFailure` 或 `Stop`。基线 reducer 可能把这些事件误判为审批已结束，造成：

- OUTBOX 审批消失；
- Agent Task 从“等待你”变成“完成”；
- 产生错误的完成提醒；
- 用户仍必须在 Codex 原生窗口处理真实请求。

P0 版本改为：

- 原生 `request_permissions` 等待状态跨同工具结束事件和 `Stop` 保留；
- 只有明确的 Provider 生命周期变化才解除等待；
- 未解除前不生成完成 Attention；
- 权威解除后再生成一次延迟完成事项；
- App 重启或新 Connector 初始枚举没有看到旧 Turn，不再被当作“已解除”的证据。

这项修复的核心不是“让 ActRealm 猜测审批结果”，而是拒绝把非权威事件当成审批结果。

### 3.4 Codex 插件安装和连接请求

P0 版本识别 `request_plugin_install`，覆盖 GitHub、Gmail、Google Drive 和其他插件安装/连接请求：

- `PreToolUse(request_plugin_install)` 创建 OUTBOX 原生等待事项；
- 任务同步进入 `awaiting_approval`；
- 卡片显示经过清理的插件名称；
- 用户被准确引导回 Codex 原生窗口；
- 匹配的 `PostToolUse` 或 failure 才作为该类请求的权威结束事件。

这类事项仍是 Provider 原生请求，因此 ActRealm 不显示虚假的“允许/拒绝”按钮。

### 3.5 完成、确认和回执文案

P0 版本不再在 Runtime 返回前显示泛化成功 toast：

- 客户端等待认证命令完成和新 snapshot；
- 完成、错误、问题、审批使用不同结果文案；
- 失败时展示 Runtime 返回的真实错误；
- root `SessionEnd` 显示“会话已结束”，不再被错误覆盖成“子 Agent 已结束”；
- Attention 解决后，只有不存在其他 blocker 才释放 Session 的等待状态。

## 4. Codex 控制能力提升与真实边界

### 4.1 M15 管理审批

P0 版本为 Codex app-server 增加版本门禁下的管理审批：

- `item/commandExecution/requestApproval`；
- `item/fileChange/requestApproval`；
- `item/permissions/requestApproval`；
- 一轮 allow/deny；
- 权限批准只返回请求中的网络/文件系统子集，不扩大授权；
- 使用已有的 3 秒延迟提交和撤回事务；
- 根据 `serverRequest/resolved` 同步 command、Attention 和 Session 状态。

### 4.2 不夸大的能力边界

直接审批仅在以下条件同时成立时可用：

1. Thread 已明确接入 ActRealm 的 managed Connector；
2. 请求真正到达 ActRealm 所有的 app-server 连接；
3. Codex app-server 版本位于已验证的 `0.144.5–0.144.x` schema 家族；
4. 请求类型和数据结构通过校验；
5. 请求仍未过期且 waiter 仍然存在。

以下情况仍只能观察和跳回 Provider：

- 任意独立运行的 Codex Desktop 会话；
- 已经在 Codex 私有连接上运行的 Turn；
- 只在 Codex 原生窗口出现、没有进入 ActRealm request-keyed 通道的审批；
- 未知版本或无法验证结构的 app-server 请求。

因此，P0 版本提升了“可直接审批”的覆盖率，但没有把所有 Codex 原生审批伪装成可直接控制。

## 5. Runtime 稳定性与恢复

### 5.1 Runtime 自动重启

P0 版本的 macOS Supervisor 增加：

- `desiredRunning` 目标状态；
- `0.5/1/2/4/8s` 有界退避；
- 连续失败熔断；
- 用户主动停止时取消自动重启；
- 重启前关闭旧 WebSocket；
- 恢复后重新建立 Runtime、Bridge 和本地控制连接。

已有实机证据中，Runtime 连续强杀恢复达到 20/20。

### 5.2 App 重启后的安全接管

当 App 异常退出但 Runtime 仍存活时，P0 版本不再盲目复用未知进程，也不任意杀进程。它只在以下条件满足时替换遗留 Runtime：

- 可执行路径匹配当前 bundled helper、已安装 helper 或已知开发 helper；或
- 移动安装路径下的 helper 具有相同 bundle ID 和签名身份；
- ad-hoc QA 环境还要求 helper 属于当前用户。

不可信 PID、路径、符号链接或签名身份不会被 ActRealm 处理。

### 5.3 Codex app-server 孤儿进程

基线使用 listener/proxy/Socket 中间层，Runtime 被 `SIGKILL` 后可能遗留 PPID-1 的 Codex listener。

P0 版本改用官方 `app-server --stdio`：

- Runtime 消失后 stdin 自动 EOF；
- Connector Drop 会 kill/wait 子进程；
- reader 和 handler 使用 Weak，避免线程/Arc 生命周期环；
- 不再为生产链路创建旧 Codex app-server Socket；
- 升级迁移只清理精确匹配 ActRealm 私有旧 Socket 的 Codex listener。

### 5.4 Runtime 状态文案有证据来源

P0 版本把“Runtime 已启动，但控制连接尚未恢复”限定为临时状态：

- WebSocket 恢复后变为“Runtime 已重新启动并恢复连接”；
- 后续再次断连会清除旧成功文案；
- 不再让一次历史重启动作永久显示成当前异常。

## 6. 实时性与性能

### 6.1 切换到其他 App 后仍实时

基线把 `NSApplication.shared.isActive == false` 当作不可见。用户点击 Codex、Claude 或 Terminal 后，即使 ActRealm 仍在屏幕上，计时和任务投影也可能停住。

P0 版本改为：

- 只要 ActRealm 窗口仍可见，即使不是 key/active App，也继续任务时间、阶段时间、额度年龄和 snapshot 投影；
- 窗口最小化或真正 occluded 时，才暂停高频 SwiftUI 动画；
- 回到可见状态时一次补齐最新 snapshot；
- Attention 和命令等重要状态仍即时投影。

### 6.2 Runtime snapshot 缓存

基线每个 WebSocket 客户端约每 100ms 重建完整 SQLite 投影。

P0 版本由单写者维护 revision-invalidated snapshot cache：

- 任意持久化 mutation 立即使缓存失效；
- 无变化读取复用最多 2 秒；
- WebSocket 仍保持 100ms cadence；
- quota polling 移出 WebSocket 读取路径，由 Runtime 自己调度。

聚焦测试记录：

- event-to-WebSocket p95：`108.976ms`；
- 两分钟 Runtime 空闲 CPU：`0.000%–0.003%` 平均值；
- Runtime 最大 RSS：`6,640–6,672 KiB`；
- 后台实时修复后的打包 UI p95：`129ms`；
- 最小化采样约 `0.0%–0.2%` CPU。

### 6.3 p95 指标仍有统计口径问题

本轮曾观察到设置页瞬时显示 `901ms`，随后同一候选恢复为 `129ms`。现有实现可能把首次连接或恢复时的持久 snapshot 当作实时样本，样本数较少时单个慢样本会直接成为 p95。

因此：

- 已修复“切换到其他 App 导致真实渲染停滞”的产品问题；
- `901ms` 不代表已确认的持续性能回退；
- 但 p95 统计仍应在后续改为：首次 snapshot 只建立基线、至少 20 个样本后显示、展示样本数，并使用 Runtime 提供的最新 mutation/received 时间。

这项统计口径清理尚未包含在 `1ced062`。

## 7. Claude 额度与睡眠恢复

P0 版本增加：

- 监听 `NSWorkspace.didWakeNotification`；
- 唤醒时取消可能假存活的 WebSocket 和 stream task；
- 调用认证的 `/api/v1/quota/refresh`；
- 拉取新 snapshot 并重建 WebSocket；
- 失败时回退到受监管 Runtime 重启；
- Runtime 在零 WebSocket 客户端时也独立轮询额度；
- 设置页新增“Provider 数据 → 主动更新额度 → 立即更新”。

主动更新的成功标准是 Claude 的真实 `capturedAt` 前进，不会因为 HTTP 请求完成就假报成功。

失败信息区分：

- 未找到 Claude 登录凭证；
- 凭证被 Provider 拒绝；
- Provider 限流；
- Provider 服务不可用。

如果 Claude 官方凭证需要 CLI 刷新，界面会明确要求：

1. 启动 Claude Code CLI；
2. 完成登录或开始一个会话，让官方 CLI 更新凭证；
3. 返回 ActRealm 再次点击“立即更新”。

OAuth access token 只在 Runtime 内存中使用，不返回 Swift、不进入 SQLite、日志、诊断或导出。

仍需真实人工完成的门禁：睡眠 1、10、60 分钟，各执行 5 轮，验证 10 秒内重连，并在 60 秒内得到新额度或明确错误。

## 8. Agent Tasks、生命周期和子 Agent

### 8.1 不再把历史回放当作当前任务

SQLite schema 11 新增内部 `last_meaningful_activity_at`：

- 只有 `SessionStart`/`SessionEnd` 的 Claude Desktop 历史回放不能让任务重新可见；
- prompt、tool、approval、question、failure、plan、sub-Agent 等真实活动才更新可见活动时间；
- 开放 Attention 始终保持关联 Session 可见；
- 首次真实 prompt 后只显示对应真实任务。

### 8.2 子 Agent 状态修复

P0 版本：

- 将 `active=0` 但 `status=running` 的旧矛盾记录迁移为 completed；
- 父 Session 结束时关闭仍 active 的旧子 Agent；
- stop/session-end 同时更新 active、status 和 stopped time；
- 不再因为子 Agent 数量为 0 覆盖 root SessionEnd 文案。

### 8.3 Provider 模式文案

权限模式和审批所有者分开存储：

- `danger-full-access`、`bypassPermissions`、`fullAccess`、`full_access`、`never` 显示全访问语义；
- `dontAsk` 显示非交互语义；
- 只有 guardian/auto-review ownership 显示自动审查语义；
- Provider 自己处理审批时，ActRealm 不声称自己已经批准。

## 9. 安全与数据影响

### 9.1 诊断脱敏

P0 版本在 Runtime stdout 被加入可见 tail 前先消费 bootstrap URL，并统一把 stdout/stderr 中的 `bootstrap=` 参数替换为 `<redacted>`。

### 9.2 本地数据迁移

P0 版本将 SQLite schema 从 10 升到 11：

- 保留已有 event、session、Attention、quota 和 usage 数据；
- 新增一个可空的 Session 活动字段；
- 只规范化自相矛盾的旧 sub-Agent 行；
- 不恢复 Runtime 重启前已经断开的审批/问题 waiter；
- 不修改用户 Hook 配置、Codex trust 选择或已安装 App。

### 9.3 控制安全

- Runtime 离线时，批准、拒绝、回答、确认、忽略和撤回按钮统一禁用；
- 跳回 Provider 仍可用；
- 未知 Codex app-server 版本 fail closed；
- Provider 原生审批只有观察能力时不显示假 allow/deny；
- permission request 不落盘、不重放。

## 10. CI 与发布流程

P0 版本新增核心 CI：

- Rust format；
- Clippy `-D warnings`；
- workspace tests；
- release build；
- Swift tests；
- RustSec；
- 产品语言检查；
- Plist 检查；
- QA package；
- codesign 和构建元数据检查。

第一版 workflow 使用 Rust 1.85，与锁定的 `libsqlite3-sys 0.38.1` 不兼容。后续两个 CI 提交已改为：

- workspace `rust-version = 1.97`；
- CI 使用 Rust 1.97；
- macOS CI 和发布门禁使用 macOS 26。

P0 版本还新增正式 macOS 发布工作流：

- Developer ID；
- DMG；
- notarization；
- staple；
- Gatekeeper；
- SHA-256；
- GitHub Release artifact。

需要区分：

- 本地 arm64 QA App、Helper 和 ad-hoc deep/strict codesign 已通过；
- 当前支持矩阵明确为 Apple Silicon / macOS 26+；
- Developer ID 正式证书、公证服务、staple 和干净 Mac Gatekeeper 安装仍需要外部环境；
- 本报告没有把 `1ced062` 的最新远端 GitHub Actions 状态写成已通过，需在 GitHub 上单独确认。

## 11. 验证结果汇总

以下是本轮现有报告记录的主要结果：

| 验证项 | 结果 |
| --- | --- |
| Rust format | 通过 |
| Workspace Clippy，`-D warnings` | 通过 |
| 完整 unrestricted Rust workspace tests | 通过 |
| Rust release build | 通过 |
| macOS Swift suite | 通过，最终记录 88 tests |
| 产品语言与 Plist | 通过 |
| RustSec | 通过，132 dependencies |
| 本地 arm64 QA 打包和 ad-hoc codesign | 通过 |
| Runtime 强杀自动恢复 | 通过，20/20 |
| Codex app-server stdio 生命周期 | 通过 |
| OUTBOX 稳定选择和优先级 | 通过 |
| Claude question/Elicitation 自动化与原生表单 | 通过 |
| 原生 Codex approval 跨 incidental event 保留 | 通过 |
| Codex plugin install/connect 生命周期 | 通过 |
| 后台可见窗口持续刷新 | 通过 |
| Runtime 状态恢复文案 | 通过 |
| 两分钟 Runtime 资源门禁 | 通过 |
| 打包 UI p95 | 通过，记录值 129ms |
| 真实 Claude 问题/Elicitation 全矩阵 | 待最终人工验收 |
| 真实 Codex M15 command/file/permission allow/deny | 待最终人工验收 |
| 睡眠 1/10/60 分钟各 5 轮 | 待人工验收 |
| App 正常退出、Force Quit、SIGKILL 各 20 轮 | 待人工验收 |
| Developer ID、公证、干净 Mac Gatekeeper | 待外部发布环境 |
| 48 小时冻结 SHA 稳定性 | 待执行 |

详细证据入口：

- [全局上线审计](ACTREALM_GLOBAL_RELEASE_AUDIT_2026-07-22.md)
- [P0 修复与复验](ACTREALM_P0_REMEDIATION_VERIFICATION_2026-07-22.md)
- [P1、CI 与 M15 修复](ACTREALM_P1_CI_M15_REMEDIATION_2026-07-23.md)
- [OUTBOX 与额度回归修复](ACTREALM_OUTBOX_QUOTA_REGRESSION_2026-07-23.md)
- [2026-07-22 证据索引](evidence/2026-07-22/00-evidence-index.txt)

## 12. 明确未包含或仍未关闭的事项

以下内容不能因为 P0 分支代码更多就写成“已完成”：

1. AR-016 的完整键盘、VoiceOver 和辅助功能矩阵没有在本轮处理；
2. AR-017 按产品决定没有作为本轮修复项；
3. 任意独立 Codex Desktop 会话仍不能保证在 ActRealm 直接审批；
4. p95 指标的首次样本和样本数口径仍需优化；
5. 真实 Claude 睡眠恢复矩阵尚未全部执行；
6. 真实 Claude AskUserQuestion/Elicitation 全类型最终回归尚未完成；
7. 真实 Codex M15 allow/deny 全矩阵尚未完成；
8. 正式签名、公证、staple、Gatekeeper 和干净 Mac 安装尚未完成；
9. 连续 48 小时冻结版本稳定性测试尚未完成；
10. 未经单独授权，不应自动把 P0 分支合并回 `agent/v1-full`。

## 13. 最终判断

相对 `50b891a`，P0 目标版本 `1ced062` 的提升不是单纯 UI 调整，而是对控制链路和产品可信度的一次系统性加固：

- OUTBOX 从“能展示事项”提升到“多事项下不易选错、不会随便假完成”；
- Claude 提问从后端支持提升到原生界面可真实回答；
- Codex 从单纯观察扩展到受版本和连接所有权约束的管理审批；
- Runtime 从一次崩溃后人工恢复提升到自动重启和安全接管；
- 后台状态从依赖前台焦点提升到可见窗口持续实时；
- Claude 额度从被动缓存提升到睡眠恢复、独立轮询和用户主动诊断；
- 历史回放、子 Agent、权限模式和 Runtime 状态文案更接近真实 Provider 状态；
- CI、发布、隐私和证据链从缺失或零散提升为可审计流程。

建议把 `1ced062` 视为 `50b891a` 之后的 **P0/P1 加固候选**。在合并回 `agent/v1-full` 或标记 v1 发布前，至少完成本报告第 11、12 节列出的真实 Provider、睡眠、退出、签名公证和 48 小时门禁。
