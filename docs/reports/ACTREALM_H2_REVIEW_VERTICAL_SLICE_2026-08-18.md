# ActRealm H2 Review v1 检查点（H2.1–H2.3）

日期：2026-08-18

分支：`agent/runtime-v2-agent-observability`

候选：ActRealm `0.1.0 (65)`，Runtime SQLite schema 32，Apple Development 签名

结论：**H2.1–H2.4 已全部通过；真实任务验收为20/20。** 当前已经
具备持久化 Turn 起点 baseline、exact/bounded/concurrent 归因、Commit 数、本地按需文件
列表/Patch、结构化验证和最后有效动作。旧 Turn 的补建 baseline 会诚实标为 late；build 64
之后的新真实 Turn 已验证在首工具前捕获；build 72–75完成标题、测试不确定性、完成后并发
归因和时态返工，并补齐Codex/Claude Code 20个真实任务门。最终证据见
`docs/reports/ACTREALM_H2_4_20_TASK_ACCEPTANCE_2026-08-21.md`。

## 1. 本地 Review API

新增 authenticated local-only：

`GET /api/v1/sessions/{id}/review`

返回：

- outcome 与 Runtime terminal reducer 来源；
- branch、短 HEAD、primary/linked worktree；
- dirty、changed/staged/unstaged/untracked 数量；
- tracked diff 的 insertions/deletions/binary 数量；
- attribution 与稳定原因码；
- 当前 Turn 最近 10 个 test/build validation；
- 最后有效结构化动作；
- limitations。

不返回：

- cwd、仓库路径和文件名；
- Diff 内容；
- Prompt、Transcript、完整命令、tool input/output；
- Provider credential 或 reply channel。

该路由没有加入 Companion/Cloud 投影。

## 2. Git 检查边界

- 只调用绝对路径 `/usr/bin/git`，不用 shell；
- 每次子命令 750 ms 超时；
- stdout 最大 1 MiB，stderr 丢弃；
- `GIT_OPTIONAL_LOCKS=0`、`GIT_TERMINAL_PROMPT=0`；
- Git 读取只在用户展开 Review 时执行，不进入 snapshot/WebSocket 高频路径；
- 路径只在 Runtime 本机进程使用。

仓库选择顺序：

1. Provider/工具明确给出的本地 workdir；
2. 会话 cwd 自身的 Git repository；
3. cwd 下最多两层、最多 128 个目录的 bounded scan；
4. 只有一个子仓库时选择；
5. 多个子仓库中只有一个 dirty 时选择，并显示该推导来源；
6. 多个 dirty 或无法唯一确定时返回 `repository_ambiguous`，不按标题猜测。

## 3. 改动归因

当前状态枚举：

- `no_changes`；
- `concurrent_changes`；
- `current_worktree_unattributed`；
- `unavailable`。

build 63 没有当前 Turn 开始时的 Git baseline，因此即使只有一个活动任务，也明确显示：

> 这是当前工作区状态；尚无 Turn 起点基线，不能把全部改动归给当前任务。

同一 worktree 有其他活动 session 时降级为 concurrent，不把共享修改归给任意单个 Agent。

## 4. 验证证据语义

- test/build 分类继续使用 Runtime 的 closed tool category；
- `ToolStarted`：running；
- `ToolFailed`：failed；
- `ToolCompleted` 只有存在结构化 exit code、success bool 或 closed status 时才是
  passed/failed；
- 缺少结构化结果时是 `unverifiable`，UI 显示“已执行，结果无法验证”；
- 不解析自然语言输出，不把工作流“成功”直接当测试通过；
- validation 按 tool call identity 合并 start/end，最多显示最近 10 项；
- `review_workdir` 与 `validation_status` 是 schema 31 的本地私有列，不进入 Session snapshot。

真实 Codex Hook 没有为本轮 Bash test/build 提供结构化退出码，因此 build 63 正确显示
“已执行，结果无法验证”，没有因为终端实际输出为 `ok` 就绕过合同。

## 5. schema 31 备份

升级前使用 SQLite online backup 创建：

`~/.actrealm/token-backups/h2-review-schema31-2026-08-18/data-before-schema31.sqlite`

- mode：`0600`；目录 mode：`0700`；
- user version：30；
- integrity：`ok`；
- SHA-256：`5175556f56cdec9ed8de6a99facc676430181fdb66531bf4a57eb2221d8b3c58`。

升级后真实数据库为 schema 31，integrity `ok`。

## 6. 真实 build 63 UI

真实 `actrealm 优化` 任务展开后显示：

- branch：`agent/runtime-v2-agent-observability`；
- HEAD：`13f08b0eb341`；
- 14 个当前工作区变更；
- `+1536 / -12`；
- `current_worktree_unattributed` 的中文解释；
- “仓库由唯一存在改动的子工作区识别”；
- test/build 均为“已执行，结果无法验证”；
- 最后有效动作来自 Runtime current-turn timeline。

主任务卡没有增加 Review 噪音；只有展开任务才按需读取并显示。另一个 Display 仓库保持
未修改且 clean，因此 bounded selection 唯一选择 ActRealm-Cloud。

## 7. 自动化

- Rust runtime unit：47 项通过；
- Rust server unit：50 项通过；
- macOS：187 项 / 27 suites 通过；
- Web + shared schema：25 项通过；
- full Rust workspace：通过；
- full workspace Clippy `-D warnings`：通过；
- release build：通过；
- language contracts：通过；
- Web `app.js` 保持低于 128 KiB 预算；
- `git diff --check`：通过；
- build 63 签名与 arm64：通过。

门禁过程中发现并修复：

1. schema 测试仍写死 version 30；更新为31后全量通过；
2. 一次已有 hook socket readiness 偶发失败，单测复跑与完整复跑通过；
3. Web `app.js` 超过 128 KiB 约1.4KiB；没有提高预算，将事实展示移入
   `agent-detail.js` 后恢复预算；
4. 真实会话 cwd 位于两个仓库的外层目录；没有按任务标题猜测，增加唯一 dirty 子仓库规则。

## 8. H2 剩余门

1. PR 只在 Provider/Git 明确提供时显示；
2. 覆盖有修改、无修改、失败、未测试、非 Git、并发、多 worktree；
3. 完成 20 个真实 Codex/Claude 任务，零虚假 passed 后才把 H2 标记为通过。

## 9. H2.2 Turn baseline

- schema 32 新增本地私有 `task_review_baselines`；
- 独立 `actrealm-review` worker 每500ms检查最多8个新当前Turn；
- Git读取不在Hook/SQLite writer路径执行，现有 Hook p95 门继续通过；
- baseline 保存repo identity、完整HEAD、branch、worktree kind、dirty和统计；
- 写入时重新查询首个`tool.started`，避免worker查询后工具已经启动造成假“及时捕获”；
- baseline按turn幂等，重启后继续使用；session/turn删除时由外键清理；
- attribution：独立干净linked worktree为exact；干净primary为bounded；起点dirty、捕获晚、
  并发或repo identity变化逐级降级。

真实当前Turn在build 64启动前已经运行，首工具时间为`1787039689432`，baseline捕获时间为
`1787040798912`，晚约1,109秒。UI正确显示“Git基线晚于首个工具事件；不能声明完整归因”，
没有升级为exact。

## 10. H2.3 本地 Diff 与 Commit

新增本机认证路由：

`GET /api/v1/sessions/{id}/review/diff`

- 默认返回最多100个tracked/untracked相对路径；
- 选中tracked文件后才读取Patch；
- Patch上限256KiB并显示truncated；
- untracked只列名称，不自动读取文件内容；
- 路径必须来自服务端文件列表，拒绝绝对路径、`..`、控制字符和路径穿越；
- Diff/路径不进入Cloud或Companion；
- baseline存在时使用baseline HEAD；否则只作为当前工作区本地Diff；
- Review显示baseline之后的Commit数量，不从自然语言或PR文本猜测。

真实build 64显示base `20db38ea3fb6`、13个当前差异、`+1561/-73`；Diff Sheet列出本轮
源码与资源文件，点击`crates/server/src/server.rs`后成功呈现本地Patch。构建产物目录作为
untracked只列名称。

## 11. schema 32 备份与更新后的门

升级前在线备份：

`~/.actrealm/token-backups/h2-baseline-schema32-2026-08-18/data-before-schema32.sqlite`

- mode `0600`，目录mode `0700`；
- user version 31；integrity `ok`；
- SHA-256：`a33d60db1f460ef70b02cab2165a4388d38ea76088ffe95fc232d1b28ddc8385`。

更新后：

- Runtime unit 47项、runtime integration 73项、server unit 52项；
- macOS 188项/27 suites；Web/shared 25项；
- full workspace、Clippy `-D warnings`、release、语言合同、128KiB Web预算全部通过；
- baseline worker 上线后的独立120秒空闲门：118样本，平均CPU `0.041%`，Runtime RSS峰值
  `25,872 KiB`，通过 `0.5% / 81,920 KiB` 预算；
- 真实数据库schema 32、integrity `ok`。

## 12. H2.4 首批真实任务证据

build 64 安装后新开始的两个真实 Codex Turn 均在独立 baseline worker 上捕获：

| 样本 | 状态 | baseline延迟 | 首工具相对baseline | 起点状态 | Review结果 |
| --- | --- | ---: | ---: | --- | --- |
| Codex Turn 89 | 完成、无工具 | 463ms | 无工具 | primary、起点已有1项改动 | bounded，不声明当前任务独占改动 |
| Codex Turn 90 | 运行中、有修改和验证 | 240ms | +8,062ms | primary、起点已有1项改动 | bounded；测试/build仍为unverifiable |

Computer Use刷新真实build 64后，Turn 90显示：

- `Turn 起点已有改动；只能显示有界工作区差异`；
- baseline HEAD `12e24ee75424`；
- 当前结构化计划为3项完成、1项进行中、5项待处理；
- Bash测试事件缺少Provider结构化退出结果，因此没有显示`passed`。

可信度补强：baseline写入后如果历史/延迟`tool.started`才进入数据库，读取Review时会重新查询
该Turn真实首工具时间。若工具发生时间早于baseline，归因会降级为
`baseline_captured_after_first_tool`，不会因为持久行最初的`first_tool_at`为空而误判及时。
Runtime新增回归测试覆盖此顺序；server归因矩阵覆盖clean primary、clean linked、起点dirty、
late和concurrent五种结果。

非Git夹具还发现并修复了一个语义错误：零个候选仓库原先会落入`repository_ambiguous`。
现在零候选明确返回`not_git_repository`；只有多个候选且无法唯一选择时才返回歧义。端点测试
同时确认非Git场景不虚构验证结果、不泄露完整工作目录或Prompt。

这些是当天的首批真实证据，不替代“7天至少20个任务”的产品门。H2在达到20/20且Codex、
Claude均覆盖前保持验收中。

H2.4 checkpoint门禁：`cargo fmt`、Clippy `-D warnings`、全workspace test、release build、语言
合同全部通过；Runtime集成测试74项、Server单元测试53项、macOS 188项/27 suites通过。

## 13. build 71/72 标题可信度与真实复验

2026-08-21 使用 Computer Use 创建Claude Code本地会话，并使用Codex官方新任务接口创建
projectless会话；随后在真实ActRealm中逐项展开任务和Review。首次复验发现两个缺陷：

- `H2.4`、`README.md`等标识符中的英文句号被错误当作句末；
- Codex新任务把`<codex_delegation>`与`source_thread_id`显示为任务摘要。

build 71修复句号边界并删除内部信封后，Computer Use进一步发现Codex的真实Prompt位于
`<codex_delegation><input>…</input></codex_delegation>`内部；删除整个block虽然不再泄漏ID，
也会丢失用户Prompt。build 72改为只提取`input`、丢弃来源元数据，并再次运行全量门。

最终真实样本：

| 样本 | 真实状态 | ActRealm标题/摘要 | Review事实 | 结果 |
| --- | --- | --- | --- | --- |
| Claude Code clean Git | `main`、HEAD `4748fc5cbc88`、0改动、未运行测试 | 完整保留`H2.4`、`README.md`、`.git` | 0文件、+0/-0、无结构化测试、不虚构passed | 通过 |
| Codex delegated非Git | projectless、0改动、未运行测试 | 从`input`提取完整`H2.4`/`v2.3.4`/`README.md`/`.env`；无thread ID | not Git、归因不可用、无结构化测试、`turn.completed` | 通过 |

Runtime数据库复核：两条会话均`response_finished`；Claude baseline HEAD与真实仓库一致；所有
session title中的`codex_delegation`、`source_thread_id`和测试私有ID计数为0。Claude Cowork
真实修改/测试任务没有产生Claude Code Hook或Review baseline，按能力边界不计入H2.4样本。

本节记录的是build 71/72阶段的4/20检查点；后续build 73–75已补齐测试不确定性、真实改动、
独立worktree、嵌套/歧义仓库和并发修改。最终口径为**20/20通过**，见独立最终报告。
