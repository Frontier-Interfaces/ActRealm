# ActRealm H2.4 — 20 个真实任务验收

日期：2026-08-21

结论：**H2.4 通过，正式样本 20/20。** Codex 与 Claude Code 均覆盖；真实任务包含有/无
改动、Git/非 Git、测试实际通过/失败但Provider证据不可验证、运行中创建仓库、独立
Worktree、单嵌套仓库、多仓库歧义和并发修改。20个样本中没有一次虚假`passed`、错误独占
归因或内部Prompt信封泄漏。

## 1. 计数口径

- “真实样本”必须来自真实Codex或Claude Code会话、真实Hook/Connector事件和真实工作区；
- 自动存在baseline不等于已验收，必须对照实际Git/测试/文件状态和ActRealm UI；
- 修复前暴露缺陷的试跑、Claude Cowork和为制造并发而创建的辅助会话不计入20个正式样本；
- Provider没有结构化退出码时，实际测试通过或失败都只能显示`unverifiable`；自然语言和终端
  输出不能升级为`passed`或`failed`。

## 2. 20 个正式样本

| # | Provider | 场景 | 真实事实 | ActRealm Review | 结果 |
| ---: | --- | --- | --- | --- | --- |
| 1 | Codex | 完成、无工具、起点已有改动 | primary/dirty baseline | bounded，不声明独占改动 | 通过 |
| 2 | Codex | 有修改和验证命令 | baseline早于首工具 | 测试/build unverifiable，无虚假passed | 通过 |
| 3 | Claude Code | clean Git、无改动、无测试 | main、HEAD `4748fc5cbc88`、0改动 | 0文件、+0/-0、无测试 | 通过 |
| 4 | Codex | delegated projectless非Git | 0改动、无测试 | 从`input`提取Prompt；not Git、turn.completed | 通过 |
| 5 | Codex C03 | 非Git创建文件、不测试 | `settings.txt`已创建 | 非Git、无validation | 通过 |
| 6 | Codex C04 | Turn中初始化Git并提交clean | main、1 commit、clean | baseline late/unborn HEAD，不声明完整归因 | 通过 |
| 7 | Codex C06 | 单嵌套Git仓库并留dirty | child main、`value.txt`修改 | 选择唯一嵌套仓库；late/bounded | 通过 |
| 8 | Codex C07 | 父目录下两个clean仓库 | repo-a/repo-b均clean | repository ambiguous，不猜仓库 | 通过 |
| 9 | Codex C08 | 版本号/文件名/URL标识符 | 无修改、非Git | `v3.4.5`、`README.md`、`.env`不截断 | 通过 |
| 10 | Codex C01R | 实际unittest通过 | exit 0只存在工具输出文本 | test executed / unverifiable | 通过 |
| 11 | Codex C02R | 实际unittest失败 | exit 1只存在工具输出文本 | test executed / unverifiable，不显示成功 | 通过 |
| 12 | Codex C05R | Turn中建Git、dirty、测试实际通过 | baseline commit + 2 untracked文件 | late/bounded；test unverifiable | 通过 |
| 13 | Claude L01 | 修改2文件、unittest实际通过 | clean baseline后dirty | 2文件差异；test unverifiable | 通过 |
| 14 | Claude L02 | 现有unittest实际失败、不修复 | source clean，新增`__pycache__` | test unverifiable；不显示成功 | 通过 |
| 15 | Claude L03 | 修改配置、不测试 | `settings.py`修改 | dirty差异；无validation | 通过 |
| 16 | Claude L04 | 自动linked Worktree、无改动 | linked、clean | 独立Worktree、0改动 | 通过 |
| 17 | Claude L05 | linked Worktree修改并测试 | linked、`slug.py`修改 | exact/linked；test unverifiable | 通过 |
| 18 | Claude L06 | 非Git父目录下唯一child仓库 | child `value.py`修改 | 选择唯一嵌套仓库并显示dirty | 通过 |
| 19 | Claude L07 | 非Git父目录下两个clean仓库 | repo-a/repo-b均clean | repository ambiguous，不猜仓库 | 通过 |
| 20 | Claude并发E | 同一worktree两Turn重叠修改 | E/F分别创建文件，Turn区间重叠 | 2变更；历史并发归因，禁止独占 | 通过 |

额外辅助会话L08A/B/C/D/F只用于制造和核对重叠，不计入20个正式样本。Claude Cowork真实
修改2个文件并运行5项测试，但没有Claude Code Hook、timeline或baseline，按能力边界不计入。

## 3. 验收期间发现并修复的问题

1. build 69/70：新Turn保留旧Review、环境上下文成为标题、嵌套仓库丢失关联；
2. build 71：`H2.4`/`README.md`被句号截断，Codex delegation信封泄漏；
3. build 72：delegation整个删除会同时丢失真实Prompt；改为只提取`<input>`；
4. build 72/73：`python3 -m unittest`未识别为测试，工具完成被误写为“成功”；改为test +
   unverifiable，并让工作流按validationStatus显示；
5. build 73/74：并发只检查“现在活跃”，任务结束后丢失重叠；改为按同工作区Turn时间区间
   复算；
6. build 74/75：并发文案使用现在时；改为“当前Turn期间存在其他任务”。

每个问题均由真实Computer Use场景发现、修复、自动化测试后再次实机复验。

## 4. 最终事实

- 新增16个正式样本：8个Codex、8个Claude Code；连同原4个为20/20；
- 当前16个新增样本均为真实会话，辅助/失败试跑不计数；
- 新样本中test/build validation事件：7个running、7个unverifiable、0个passed、0个failed；
- 0个session title包含`codex_delegation`、`source_thread_id`或测试私有ID；
- SQLite schema 34，`integrity_check = ok`；
- 任务标题、项目、Provider、模型缺失降级、完成状态、Git结果和工作流均经真实UI检查；
- 没有点击替用户确认Outbox，新任务按既有`autoHideAt`规则自行离开活跃面板。

## 5. 自动化与实机门

- `cargo fmt --all -- --check`：通过；
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过；
- `cargo test --workspace --offline`：通过；
- `cargo build --workspace --release --offline`：通过；
- Runtime语言合同：通过；
- macOS：191项测试通过；
- Web agent detail：5项测试通过；
- Apple Development签名、arm64、Info.plist：通过；
- Computer Use：标题、Review、测试不确定性、Worktree、嵌套/歧义和完成后并发文案通过。

## 6. H2之后

H2完成不表示Provider突然提供了测试退出码；unverifiable仍是正确能力边界。下一实施阶段是
H3活跃/历史分离、确认与自动隐藏语义、最小历史中心，然后进入H7本地诊断。iPhone、Watch
和Claude Cowork继续保持Deferred，不参与当前阶段。
