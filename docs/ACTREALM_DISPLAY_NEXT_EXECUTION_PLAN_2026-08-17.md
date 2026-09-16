# ActRealm → Display 后续完整执行计划

日期：2026-08-17

适用分支：`agent/runtime-v2-agent-observability`

最后更新：2026-08-18

当前本机候选：ActRealm `0.1.0 (64)`，Apple Development 签名，Runtime SQLite schema 32。

> 产品升级主计划已迁移到
> `docs/ACTREALM_HUMAN_CONTROL_PLANE_UPGRADE_PLAN_2026-08-18.md`。本文件继续负责 A3
> 收尾和 ActRealm → Display 双仓库交付细节；Review、事实可信度、历史、Checkpoint、
> iPhone/Apple Watch 远程审批的优先级与验收以新主计划为准。

## 1. 当前基线

已经完成：

- R0：ActRealm Runtime protocol v2、Companion 授权、Codex 真实闭环和本机候选安装。
- R1：当前 Turn 任务流程、规范化工作流、工具生命周期合并、分页、Bash 降噪和真实
  subagent 边界。
- Token/额度基础：日/周/累计、年度热力图 hover、输入/输出/cache/reasoning 拆分、
  Codex Pro/Spark 规则、Claude/Codex 额度和重置时间。
- 2026-08-17 Token 可信度审计已发现历史回补、费用完整性和 Agent 时间周期口径的 P0/P1
  问题；当前图表功能存在不代表历史数值已经可信，A3 必须先通过数据真相门再做视觉收口。
- R2.1/A1：已完成任务“确认后隐藏/延时自动隐藏”策略；自动模式支持 5/15/30/60
  分钟，只处理 Runtime 已验证的 completion。build 38 已把“提醒已读”和“任务到期隐藏”
  拆成独立持久化状态，并保持任务创建时的 deadline 不受后续设置切换影响。
- A1 排序与焦点稳定性：错误/阻塞、可操作授权、Provider 原生等待、问题、运行和完成使用
  固定优先级；同级运行任务不再因普通工具事件频繁换位；Web 保持当前 Attention，只有
  更高优先级请求到来才切换。
- 全工作区 Rust、macOS 179 项测试、Web/Schema/语言合同与本机 build 38 安装验证通过。
- A2 Agent 看板事实层已完成并安装为 build 39：任务/项目/模型/安全摘要、当前动作和安全
  target、任务/Turn Token、Token 来源与完整性、当前 Turn 计划、语义工作流、真实 subagent
  空态以及额度 reset 字段级来源均已收口。Codex、dewu 和真实 Claude Code smoke 已用本机
  UI 逐字段验证；MCP 工具在分页后仍保留 `MCP server.tool` 安全身份。
- A3 第一检查点已安装为 build 43：Codex fork/subagent 继承累计基线不再入账；
  `token_usage_session_days` 成为 canonical ledger，daily/model 表由它事务内重建；旧数据可由
  完整历史记录自动降低和纠正。7 月 27 日从 `2,345,247,004` 修正为 `58,629,794`，三套
  汇总在历史回补过程中持续满足求和恒等式。
- Token 热力图已区分费用未知与 `$0`，使用 P95 + log 稳健分档；日/月/累计的“任务观测
  时间”分别使用对应本机日历周期。热力图、估算费用、任务观测时间均已接入显示设置。
- A3 第二检查点已安装为 build 45：总计、模型、Provider 和逐日数据均携带 priced/unpriced
  Token；部分费用显示“至少 $X”及定价覆盖率。首次历史回补期间保持“历史重建中”，只有
  每个已发现日志都追平当前 EOF 后才标为 verified，并且未追平时不生成虚假的审计时间。
- build 46 已完成独立 Token JSON/CSV 数值导出；只导出 day/provider/model 与 Token、费用、
  消息数和 coverage 数值，不含 session/task ID、Prompt、路径、命令、工具内容或回复。设置
  页面两种格式均已用真实 Runtime 打开保存面板验证，测试结束后取消，未写用户文件。
- build 48 已把日/模型费用的 `cost_kind` 与 `pricing_source` 冻结进 canonical ledger：Token
  事实未变时保留原计算费用和原价表来源，Provider 日志中的官方费用可以升级计算值；旧
  schema 28 费用诚实标成 `legacy_pre_v29`，不会伪装成当前价格。快照、UI、JSON/CSV 均能
  审计来源，零 Token/零费用行不会虚增来源数量。
- build 49 已实现 bounded 本机异常审计：projection 不一致、未来日期、负数和极端单日
  跳变进入 `suspect` 并给出原因；suspect 日期不参与峰值结论。价格缺失只作为 warning，
  不否定仍可用的 Token 总量。真实 UI 已验证可展开原因卡与隐私边界。
- build 50 已把“Agent 执行时间”与“任务观测时间”分开：前者只累计思考、工具运行和
  上下文压缩，排除授权/问题等待与 Runtime 离线时间；两者均按日/月/累计分别聚合，并已
  在真实 macOS UI 和独立设置开关中验证。
- build 51–57 已完成 A3 的整代 shadow/原子发布门：扫描未追平时继续显示上一代账本，
  追平后才在同一事务内更新 canonical、重建两套 projection、逐字段验证并提交。超过读取
  上限的真实 Codex 历史会得到 `partial`，不伪装 verified；Codex 两阶段解析、2 MiB 单行
  上限和共享缓冲把真实扫描 Runtime 峰值从失败的 `115,296 KiB` 降到 `25,296 KiB`。
  build 57 将稳定的 partial 状态统一显示为“历史数据部分可用”，不再误导为仍会自动补齐。

仍未完成：

- A1/A2 已完成；当前继续 A3 Token 数据真相、价格、额度、设置与性能收口。Claude Desktop
  与其内嵌 Claude Code `2.1.229` 的真实成功链路已覆盖任务、工作流、Token 与额度；重置
  时间在官方响应为 `null` 时继续诚实显示“Provider 未提供”。
- A3 的 fork 去重、唯一账本、未知/部分费用、定价覆盖率、价格来源冻结、稳健热力图、
  周期观测时间、纯执行时间、可解释 suspect/异常原因、隐私安全数值导出和 shadow/原子
  重建门与真实 partial UI 复核已落地；仍需完成 2 小时 RSS、数据库增长和睡眠唤醒护栏，不能把 build 57 当成
  A3 全部完成。
- Display 尚未接入本轮 `autoHideAt`，也尚未按最终自适应副屏方案实现。
- 长时间运行、睡眠唤醒、Display 重连和不同屏幕尺寸尚未完成正式 soak。

## 2. 不可破坏的约束

1. **先 ActRealm，后 Display。** ActRealm 阶段全部通过并由用户确认满意后，才修改
   Display 仓库。
2. **Runtime 是事实源。** UI derived 层只做排序、分组、格式化和布局，不猜测 Provider
   生命周期、完成、授权能力或计划进度。
3. **保留原 Agent 页面。** Agent 看板是从原页面进入的独立工作面，不替换已有入口。
4. **Display 不展示 Team。** Team 数据、页面和通知不进入副屏范围。
5. **运行超过 30 分钟不是隐藏条件。** 只有 Runtime 验证完成后，任务才可按确认或策略
   隐藏。
6. **隐藏、清除展示、停止任务、删除历史是四种不同语义。** UI 不得混用。
7. **不默认展示敏感内容。** 不展示完整 prompt、完整路径、命令参数、tool input/output、
   transcript、凭据或隐藏推理；只使用安全摘要和 basename。
8. **所有尺寸自适应。** 使用容器可用空间和内容优先级，不绑定 2880×864 或任何设备
   型号。
9. **现有功能不得回归。** Token、额度、Agent Focus、授权、任务跳转、Companion 和
   数据迁移必须持续通过原有测试。

## 3. 总体顺序

| 阶段 | 范围 | 目标 | 预计工程日 | 进入下一阶段的门槛 |
| --- | --- | --- | ---: | --- |
| A0 | ActRealm | build 37 体验与当前改动检查点 | 已完成 | build 37 问题已复现并进入 A1 |
| A1 | ActRealm | Attention、排序、隐藏与删除语义收口 | 已完成 | build 38 并发任务/请求零重复、零误隐藏 |
| A2 | ActRealm | Agent 看板信息来源与当前 Turn 展示收口 | 已完成 | build 39 真实 Codex/Claude 信息准确、空态诚实 |
| A3 | ActRealm | Token 数据真相、额度、设置与性能收口 | 进行中 | 全量重建可解释，数据不重计，UI/内存/CPU 达到护栏 |
| A4 | ActRealm | 诊断、恢复、真实设备验收与候选发布 | 4–6 | 用户明确确认 ActRealm 满意 |
| D1 | Display | Companion/derived 数据层接入 | 3–5 | 与 ActRealm 快照契约一致，无重复推断 |
| D2 | Display | 多版自适应看板原型和视觉定稿 | 3–5 | 用户在真实副屏选择方案 |
| D3 | Display | 聚焦、Outbox、任务队列和详情交互 | 5–8 | 真实多任务闭环稳定 |
| D4 | Display | 设置、删除、额度、恢复与性能对齐 | 4–6 | 两端语义一致、资源稳定 |
| D5 | 双端 | 尺寸矩阵、7 天 soak、文档与发布候选 | 5–7 + soak | 发布门禁全部通过 |

工时是单人临时估算，不是交付承诺；每阶段结束根据真实问题重新估算下一阶段。

## 4. A0 — build 37 检查点（已完成）

目标：先让用户实际体验本轮完成任务隐藏策略，不把尚未确认的交互继续复制到 Display。

检查项：

1. 默认“确认后隐藏”：完成任务持续保留，点击“确认完成”后从活动任务和 Outbox 消失。
2. 自动模式：5/15/30/60 分钟可选；点击“知道了/确定”只清除 Outbox 提醒，任务继续以
   已完成状态显示，只有到达原定时间才隐藏。Outbox 不持续承担倒计时展示。
3. 长任务、授权、问题、错误和断线任务不会按时间自动隐藏。
4. 隐藏后 Token、时间线和历史数据仍可查询；任务清除按钮仍维持独立语义。
5. 重新启动 Runtime/macOS 后设置和 deadline 恢复一致。

交付：用户反馈已转化为 A1；build 38 已安装供真实体验。本轮尚未 commit/push。

## 5. A1 — Attention、任务排序、隐藏与删除

目标：多个 Agent 同时运行时，用户永远先看到真正需要处理的任务，同时不会因新事件
导致列表频繁跳动。

实现任务：

1. 把排序键固定为：需要处理的错误/阻塞 → 可直接处理授权 → Provider 原生等待 →
   问题 → 当前运行 → 已完成待确认；同级等待项最早优先，普通运行任务最近活动优先。
2. 正在查看或展开的运行任务在同级中保持稳定，不因 routine tool event 每秒换位；出现
   更高优先级 Attention 时才允许聚焦切换。
3. 同时出现多个 Attention 时只聚焦一个，其余进入队列；处理完成后取下一个，不并行
   弹出多个处理面板。
4. Outbox 在没有待处理项时完全收起；完成待确认是否进入 Outbox 继续由通知设置控制。
5. 把 completion 的“提醒已读”和“任务可见性”拆成两个持久化状态。自动模式点击
   “知道了/确定”只关闭 Outbox Attention，不清除 `autoHideAt`，任务到期后才从 Agent
   Tasks 隐藏。
6. 手动模式继续使用“确认完成并隐藏”；自动模式使用“知道了/确定”，避免相同按钮在
   两种策略下产生不同却不可见的结果。
7. 明确“提醒已读”“确认并隐藏”“从列表隐藏”“清除任务展示记录”的文案、API 和
   resolution；任何展示动作都不停止 Claude/Codex，也不删除 Provider 会话。
8. 增加诊断信息：`reminder_acknowledged`、`ack_hidden`、`auto_hidden`、
   `superseded_by_activity`，并保持 reminder reason 与 visibility reason 分离。
9. Runtime 断开、Attention stale/expired 或 reply channel 消失时立即禁用动作，保留“回到
   原应用”路径。

测试矩阵：

- 1/3/6 个并发任务；0/1/5 个 Attention。
- 同时授权、提问、错误、完成；同一 request 重放；Runtime 重启；两个客户端竞争确认。
- 一个任务连续运行 2 小时无普通事件；不能自动隐藏或被标成完成。
- 自动模式确认后 Outbox 消失、任务仍显示、deadline 不变；到期才隐藏。
- 自动模式不确认时，到期同时关闭提醒和任务；不能残留幽灵 Outbox。
- 自动/手动模式互相切换、两个客户端确认、确认与到期竞争的幂等和历史保留。

验收：零重复请求、零 stale 提交、零运行任务误隐藏、列表没有 routine-event 抖动。

执行状态（2026-08-17）：代码、迁移、自动化测试、签名和本机安装已经完成；当前进入真实
UI 验收。自动模式点击“知道了”后 Outbox 应立即消失，但任务仍显示到原 deadline；手动
模式点击“确认完成”后立即隐藏。A1 验收通过前不开始 Display。

## 6. A2 — ActRealm Agent 看板与信息来源

目标：先在 ActRealm 把每个字段的来源、缺失状态和层级做正确，再交给 Display 消费。

### 必须展示的核心层

- 任务名称、项目名称、Agent 来源、当前模型、Runtime/执行状态。
- 安全任务摘要（不是完整原始 prompt）。
- 当前动作、当前文件 basename、任务总时长、当前阶段时长。
- 当前任务 Token、当前阶段/Turn Token；Provider 没有可靠边界时明确“不支持/暂无”。
- Codex、Claude 额度与重置时间；Token 与额度继续作为两个模块。每个重置时间还要显示
  来源质量：官方 StatusLine、官方 OAuth、明确标记的本机预计或 Provider 未提供。

### 展开后展示

- 当前 Turn 的任务流程/计划步骤与准确状态。
- 当前 Turn 的规范化工作流：具体工具名、语义类别、状态、安全目标、持续时间。
- 上下文占用、输入/输出/cache/reasoning 细分、数据来源与完整性。
- 真实 subagent；没有 Provider 事实时不显示虚构数量。

### 来源优先级

1. 任务名称：用户/Provider 正式标题 → 当前 prompt 的安全摘要 → 项目 + Agent 回退标题。
2. 项目名称：受信任 cwd 的最后一级安全标签 → Provider workspace 名 → “项目未知”。
3. 当前文件：Provider 明确 path 字段经 basename 脱敏；不从 Bash 命令猜测。
4. 当前动作：规范化事件 phase/status + 安全工具名；不能把所有工具统一显示成 Bash。
5. 任务流程：只绑定当前 Turn；新 Turn、completion、failure 后旧流程不继续冒充当前。
6. 工作流：start/update/end 合并；并行同名工具使用 invocation identity 精确配对。

### 滚动与空态

- 计划和工作流独立滚动，支持向前分页；用户向上查看时暂停自动滚到底部。
- 无计划时分别显示：Provider 不支持、当前 Turn 尚未提供、计划已经完成、数据暂不可用。
- 无工作流时分别显示：等待首个工具事件、当前阶段没有工具、Provider 未提供、连接中断。
- 不展示 `0/0`、假百分比或“未知工具”来制造虚假精确度。

验收：用当前 ActRealm 任务、dewu 长任务、一个真实新 Codex 任务，以及当前已经运行的
Claude Desktop/内嵌 Claude Code 任务逐字段核对来源；不依靠演示数据通过。Claude 实测
至少覆盖任务名、项目、模型、当前动作、工具生命周期、当前文件、计划、工作流、Token、
上下文、额度、提问/授权、完成和跳转降级。

## 7. A3 — Token、额度、设置与性能收口

目标：先让 Token、费用和 Agent 时间可追溯、可重建、可解释，再保留 Token Monitor 值得
借鉴的可读性并完成资源成本和设置收口。2026-08-17 审计报告见
`docs/reports/ACTREALM_TOKEN_DATA_TRUST_AUDIT_2026-08-17.md`。

任务：

1. 核对 input、output、cache read、cache creation、reasoning 和 unclassified 是不重叠口径；
   Provider 不提供的字段显示不可用，不用 0 代替未知。
2. 热力图保持逐日 hover，日/周/累计切换使用真实聚合；趋势图补齐空日但不伪造历史。
3. 每个模块显示数据来源、覆盖起点、最后成功时间、完整/部分状态。
4. 保持 Codex Pro/Spark 分离：只有 Provider 返回 Pro 和 Spark window 才显示 Spark。
5. Claude 与 Codex 使用同一展示结构，但不强行假设相同额度窗口。
6. 所有新字段和图表模块接入“设置 → 显示”，旧 Web 设置保存不得覆盖新设置字段。
7. 图表与 session 明细按需加载；增量扫描，不在首页一次读取全部日志。
8. 增加本地 CSV/JSON 数值导出，默认不含 prompt、路径、命令或回复内容。
9. 将当前编译期内置模型价格表升级为可审计的版本化定价服务；价格更新、离线回退、历史
   冻结和覆盖率必须按本计划的“模型定价与费用可信度专项”执行。

### Token 数据真相专项（A3 的第一阻塞门）

已确认的现状：

1. 7 月 27 日修复前详情聚合为 `2,345,247,004` Token。两个 Codex 子 Agent 的首条
   `total_token_usage` 都是同一个继承自父任务的 `1,127,901,221`，当前 collector 将这两
   个基线分别从 0 计入，造成约 22.55 亿重复量。按逐轮 `last_token_usage` 做审计性粗算，
   当天更接近约 9000 万；最终数值必须由修复后的全量重建产生，不能直接手工改峰值。
2. 修复前数据库三条汇总路径不一致：`token_usage_daily` 约 67.94 亿、
   `token_usage_session_days` 约 74.23 亿、`token_usage_daily_models` 约 56.91 亿。UI 主要
   使用 session-day，而“记录起点/同步成功”等元数据部分来自另一条路径，用户无法判断
   哪条才是事实。
3. 费用不是订阅账单，而是按模型、未缓存输入、缓存读取/写入和输出单价计算的 API 等价
   估算，因此与 Token 不应被强制画成完全相同深浅；但当前还把未知费用当作 0，属于明确
   错误。7 月 27 日仅 `141,817` 个未知价格 Token 就让整日费用变成 nil，热力图随后把
   nil 转成 0，掩盖了 99.994% 已定价覆盖。
4. Token 与费用各自用全年最大值做四档线性归一化；异常峰值会把其余日期全部压到最浅
   档，且两套独立最大值没有稳定图例，难以比较。
5. “Agent 时间”当前是所有 Turn 从开始到最后事件的全历史时长总和，日/月/累计三个
   周期都读取同一个 `activeTimeSeconds`，所以切换必然不变。它还包含等待用户、授权和
   工具等待，不应直接命名为“Agent 活跃时间”。
6. `collectionState=ready` 只表示扫描成功，不表示数字通过一致性校验；服务端的
   `partial/unavailable/scanning` 也没有被 macOS 状态文案完整区分。

修复顺序：

1. **先备份与审计，不原地修数字。** 对现有数据库做可恢复备份，生成只含数值、来源
   fingerprint 和异常原因的本地审计报告；旧表保留到新旧对账通过。
2. **定义唯一口径。** 为 Codex/Claude 及其版本明确 total、input、cache read/create、
   output、reasoning 的包含关系；同一 Provider 请求/事件只能入账一次。Runtime 生成
   canonical daily ledger，日/月/累计、Provider、模型和热力图都从该账本派生。
3. **修 Codex fork/subagent。** 读取 `session_meta.source.subagent`、`forked_from_id` 和
   `parent_thread_id`；历史回补优先累加事件级 `last_token_usage`。只有能证明 session-local
   的累计计数才能使用首值；继承/全局 cumulative 首值只作为 baseline，不能作为新消费。
   旧版本缺少事件级值时只计可验证增量并标记 partial，不能猜整段用量。
4. **修 Claude/多来源去重。** transcript、StatusLine cache 和实时 session projection 按
   provider session、事件身份、日期和模型合并；相同数据不能因来源不同重复。模型别名
   规范化后再定价，但未知模型不套用相近模型价格。
5. **版本化价格与覆盖率。** 费用保存价格版本、来源、priced token、unpriced token 和
   coverage。部分可估价时显示“至少 $X · 覆盖 Y%”，完全未知显示“费用未提供”，绝不
   显示 `$0`。`claude-opus-5` 已由 2026-08-18 的 Models.dev 精确目录与离线快照补齐；
   仍未精确匹配的模型继续保持 unknown。
6. **重建而非叠加修补。** 新 collector 与 fixture 通过后，从原始日志重新生成 shadow
   ledger，逐日对账，再原子切换；cursor、session-day、Provider/model/day/total 必须满足
   可证明的求和恒等式。迁移可回滚，不删除原始 Agent 日志。
7. **热力图语义重做。** Token 与费用使用完全相同的日期网格、缺失标记和稳健分档算法，
   但各自按真实数值着色，不人为强制同色。使用 log/P95 或分位数抑制单个峰值；超过尺度
   的真实峰值保留标记和精确 tooltip。费用未知/部分使用专门纹理，不能当成零。
8. **解释合法差异。** tooltip 同时展示 Token、API 等价估算费用、缓存命中占比、模型/
   Provider、定价覆盖率与数据质量；缓存读取便宜、输出昂贵或模型单价不同造成的深浅
   差异必须可解释。
9. **Agent 时间按周期重建。** 将“任务经过时间”和“Agent 执行时间”分开：前者包含
   等待，后者只累计 Runtime 处于 executing/tool-running 的区间。跨午夜按本机日历拆分；
   日/月/累计分别聚合。并发任务的“Agent 执行时间”允许相加，如展示墙钟占用则另给
   重叠去重指标，不能混称。
10. **数据质量成为产品状态。** 增加 `verified/partial/suspect/rebuilding/unavailable`、
    最后审计时间、来源覆盖和异常计数；检测继承基线、异常跳变、求和不一致、未来日期、
    价格缺失。suspect 数据默认不参与“峰值”结论，并允许用户展开查看原因。
11. **修状态文案与设置。** macOS/Web 都完整处理 ready/scanning/partial/unavailable/
    rebuilding；设置可控制 Token、费用、Agent 时间、数据质量和定价覆盖显示，不能关闭
    警告后把未知变成可信。

专项回归与验收：

- 两个从同一父任务 fork、首条累计值相同的子 Agent fixture，只计算各自事件级增量一次；
  父子并发、Runtime 重启、日志重复发现和历史/实时交接均不重计。
- 同一份 fixture 重建两次 byte-for-byte 得到相同 canonical ledger；day/provider/model
  求和与 month/total 完全一致，不再存在三套不同总额。
- 7 月 27 日修复前后保留审计差异；新值能逐条回溯到安全的 token event，不再包含两个
  `1,127,901,221` 继承基线。约 9000 万只是审计预估，不作为硬编码验收值。
- cost 为 nil、部分覆盖、100% 覆盖、不同模型/缓存比例均有测试；未知绝不渲染为 0，
  tooltip 的覆盖率与加权价格可复算。
- 异常峰值不会把其他 364 天全部压成同一浅色；Token/费用网格一致，合法颜色差异有
  cache、模型与价格解释。
- 日/月/累计的 Agent 执行时间分别变化；跨午夜、等待授权、并发任务、运行中 Turn、
  completion 和睡眠唤醒口径明确且可复算。
- shadow rebuild、原子切换、回滚、旧数据库升级、5,000 session 性能和原有 Token UI
  交互全部通过后，A3 数据真相门才算完成。

阶段检查点执行状态（更新至 2026-08-18，build 59）：

- 修改数据库前已通过 SQLite online backup 保存 mode 600 的可恢复副本；完整性为 `ok`，
  SHA-256 为 `1a0482e730a88acefd8acd53773c1235f6ce2415bc17341872d1eccaf2def710`。
- 已实现 inherited subagent session-local 计量及回归 fixture；父任务 replay 只建立 baseline，
  子任务后续增量才进入账本。7 月 27 日确定性重建结果为 `58,629,794` Token。
- `token_usage_session_days` 已成为唯一事实账本；daily/model projection 每次完整历史替换后
  在同一事务内重建。build 43 真实回补期间三者多次抽查完全相等，且 SQLite integrity 为
  `ok`。
- 热力图费用 unknown 不再冒充 `$0`，异常峰值使用 P95 + log 稳健分档；hover 保留精确
  日期与数值。日/月/累计的任务观测时间已在真实 UI 分别验证为不同结果。
- build 45 进一步加入 priced/unpriced Token 与部分定价下限，真实 UI 验证为“至少
  US$2,175.05 · 定价覆盖 90.4%”（扫描中的瞬时值）；历史未追平时页头明确显示“历史重建
  中”，不会因某一轮 bounded scan 成功就提前显示 verified。
- build 46 新增 `token_usage_numeric` JSON 与平面 CSV；两种格式从 canonical
  `token_usage_session_days` 按 day/provider/model 聚合。JSON 写明费用下限、reasoning 是
  output 子集和 unclassified 口径；CSV 保留 priced/unpriced 原始数，避免只导出舍入百分比。
- build 48 将 Runtime schema 升到 29，账本逐行保存 `cost_kind`/`pricing_source`。同一 Token
  事实收到新内置价表时不静默重算；Provider 官方费用可升级计算值，Token 事实修正时费用
  可随事实替换。JSON/CSV 同步输出费用种类和来源，UI 总计页显示来源数与冻结语义。
- schema 29 写入前另存 online backup：
  `~/.actrealm/token-backups/a3-price-freeze-build47-2026-08-17/data-before-schema29.sqlite`，
  integrity `ok`，SHA-256 `63b0b3cbaa20b201c7066223183c3068a98a693eefb546b61f11de661af1f930`。
- build 49 对 canonical、daily 和 model projection 做本机一致性审计，并检测未来日期、负数
  与有足够样本时的极端单日跳变；`suspect` 日期不参与峰值结论。价格缺失保持 warning，
  不把 Token 总量误判为不可用。异常原因可在 Token 页展开，并进入隐私安全 JSON 导出。
- 真实本机账本 integrity `ok`、schema 29；build 49 页面只发现价格覆盖 warning，没有发现
  suspect。build 48 已移动到废纸篓保留回退。
- build 50 将 Runtime schema 升到 30，新增 `agent_execution_intervals`。等待授权会结束执行
  区间，恢复后重新开始；运行中的 open interval 在 Runtime 重启时按最后事实事件闭合，
  不累计离线时间。历史数据只从规范化事件保守回建。
- schema 30 写入前另存 online backup：
  `~/.actrealm/token-backups/a3-execution-time-build49-2026-08-17/data-before-schema30.sqlite`，
  integrity `ok`，SHA-256 `f73aaf602797e33f8e7ce54c0714605414490c120d538c832b909dd919b4bc22`。
- 真实 UI 已验证 Agent 执行/任务观测：日 `11h 47m / 12h 2m`、月
  `45h 42m / 65h 49m`、累计 `77h 14m / 107h 10m`；显示设置含独立的 Agent 执行时间
  开关。build 49 已移入废纸篓保留回退。
- build 51–56 完成提交前 projection invariant、generation 故障回滚、EOF 与历史完整性
  分离，以及真正的整代 shadow 发布。build 56 回扫前总计 `4,425,062,023` 在约 6 分 20 秒
  扫描期间保持不变，追平后一次切换到 `4,438,400,572`；之后的增量来自仍在运行的真实
  任务。所有采样的 canonical/daily/model 总计完全相等。
- 本机 2.58 GB Codex 与 18 MB Claude 来源上，Codex 两阶段解析只完整读取用量事件；
  2 MiB 单行上限与跨文件共享缓冲不降低 10 MiB 每轮吞吐。真实 Runtime 扫描峰值从
  build 53 的 `115,296 KiB` 降到 build 56 的 `25,296 KiB`。
- 最终源码 120 秒独立门禁为平均空闲 CPU `0.009%`、Runtime RSS 峰值 `7,888 KiB`；
  macOS Swift 183 项 / 26 suites、Rust workspace、Clippy、release、语言合同和签名均通过。
- build 57 在真实历史扫描期间继续保持三个总计完全相等，并在追平时一次性发布；8 分钟
  48 个实机采样点中 mismatch 为 0，Runtime RSS 峰值 `23,936 KiB`。打开过完整 Token
  仪表板和无障碍树后，SwiftUI 进程 RSS 从峰值 `190,160 KiB` 回落到 `134,608 KiB`，短时
  未见持续增长；这组数据只作为短时观察，不替代 2 小时资源门。
- build 57 的真实主卡片与 Token 仪表板均已复核为“历史数据部分可用”；扫描过程中仍显示
  “首次扫描中，统计仍会增长”，追平后才切换到 partial 文案。验收未处理用户 Outbox。
- build 58–59 删除了没有真实金融语义的 Token K 线及其 OHLC 派生；总览不再同时
  绘制易重复的“模型/Agent”两张榜单，改为一张可切换的“工具来源/按模型”卡片，趋势图
  复用同一维度选择。工具来源只表示 Codex/Claude 采集来源，不再误称独立 Agent。
- build 58–59 启用固定 `https://models.dev/api.json` 的低频价格目录：本机用量历史
  追平后才请求，10 秒超时、16 MiB 上限、成功缓存 24 小时、失败至少退避 1 小时；缓存
  写入 `~/.actrealm/cache/models-dev-pricing.json` 且权限为 `0600`。请求不携带账户、
  session、Prompt、项目、路径或本机用量，损坏/缺失/断网时回退最后有效缓存或内置快照。
- 动态目录只接受 Anthropic/OpenAI 的精确模型 ID、有限的已审核 alias 和合法非负
  input/output/cache 单价；禁用 fuzzy/包含匹配。Provider 日志费用仍优先，已落账费用仍按
  原来源冻结；以前为 null、现在获得精确价格的同一 Token 事实允许补价。partial 历史的
  已观测 Token 组件也可估价，不再仅因历史前缀缺失就把已知模型的费用整段置空。
- build 59 真实历史追平后成功生成 `3,934,279` 字节、mode `0600` 的 Models.dev 缓存；
  `claude-opus-5` 的 `168,267,905` Token 已全部补价。总计页实测为
  `4,245,518,296` Token、至少 `US$2,559.01`、覆盖 `90.1%`、5 个价格来源；三层账本总计
  相等且 SQLite integrity 为 `ok`。`codex-auto-review` 和缺少输入/输出组成的旧 partial
  行继续保持 unknown，不制造伪精度。新鲜但损坏的缓存会重新拉取，不会卡住一整天。
- Rust workspace 全量测试、Clippy、release build、Swift 183 项测试、语言合同、签名和本机
  UI 已通过；详细证据见
  `docs/reports/ACTREALM_A3_TOKEN_TRUTH_CHECKPOINT_2026-08-17.md`。
- 本检查点已完成 A3 的 shadow/原子门、短时资源门与真实 partial UI 文案复核；2 小时
  RSS、数据库增长和睡眠唤醒仍是后续阻塞项，因此 A3 继续保持“进行中”。

### 模型定价与费用可信度专项

当前状态与边界：

1. ActRealm 以 `crates/usage/src/pricing_snapshot.json` 的编译期快照作为离线基线，并在
   生产 Runtime 中低频读取 Models.dev 固定 API 的精确 Anthropic/OpenAI 目录。当前实现
   缓存最近一次通过结构与数值校验的原始目录；断网、超时、空响应或损坏缓存不会覆盖
   基线。长上下文/批处理阶梯还没有足够的逐请求事实支持，因此当前仍使用 base rate，
   不能把费用称为 Provider 账单。
2. 当前计算按未缓存输入、输出、cache read、cache creation 分别计价；Codex 的 cached
   input 先从 input 中扣除再按缓存单价计算，Claude 的 cache read/create 分开计算。
   reasoning 当前包含在 output 中，不额外重复收费。数值语义是“API 等价估算”，不是
   Codex/Claude 订阅账单或用户实际扣款。
3. Token Monitor 本身不维护完整单价表，而是通过 Tokscale 获取用量和费用。Tokscale 会
   聚合 LiteLLM、OpenRouter、models.dev 等目录，使用约一小时缓存、失败时回退陈旧缓存，
   并支持用户自定义精确价格。它适合发现新模型和补充覆盖，但第三方目录、代理商价格和
   fuzzy model match 不能未经审计直接成为 ActRealm 的费用事实。

实施规则：

1. **固定可信来源优先级。** 同一用量事件按以下顺序选择费用：Provider 官方随事件返回的
   `costUSD`/等价字段 → 用户对精确 provider + model + version 的显式覆盖 → 根据官方
   定价页维护并审核的 ActRealm 版本化快照 → 已审计第三方目录的精确匹配 → unknown。
   Claude transcript 的完整官方 `costUSD` 保持最高优先级；Codex 没有官方事件费用时才
   进入估算链。来源不得按“哪个价格更高/更低”动态选择。
2. **动态发现、审核发布。** 后台低频获取候选目录，使用 ETag/Last-Modified、超时、退避
   和本地缓存；候选价格先进入 staging，检查单位、币种、provider、上下文阶梯、缓存字段
   和异常涨跌，通过签名/校验和与人工或测试门后才发布为新的只读 pricing snapshot。
   外网失败继续使用最后已验证版本并显示陈旧时间，不阻塞 Runtime，也不把空响应发布为
   新版本。
3. **只允许确定性模型解析。** 保存原始 provider/model，同时建立版本化 alias 表；只接受
   精确名称、明确 provider 前缀、官方版本别名和经过 fixture 验证的规范化。默认禁止用
   Levenshtein/fuzzy、名称包含关系或“最相近模型”套价；同名跨 Provider 不合并。无法确定
   时保持 unknown，不能用类似模型价格制造精确费用。
4. **完整价格 schema。** 每条价格至少包含币种、每百万 Token 的 input/output/cache
   read/cache creation、适用 Provider、模型版本、有效起止时间、上下文/批处理阶梯、来源
   URL、采集时间、审核时间和 snapshot id。Provider 未单列的 reasoning 继续明确归入
   output；未来若 Provider 独立收费，通过 schema 版本迁移，不能在旧账上重复计费。
5. **历史费用冻结。** canonical ledger 的每条费用保存 token breakdown、实际采用单价、
   price source、snapshot id、估算/官方标记和 coverage。新价格只影响其生效点之后的新
   事件，不静默重写历史。若用户主动选择“按新价格重新估算”，必须生成独立视图/导出，
   显示重估版本和差异，原始历史账本保持可回溯。
6. **部分覆盖不等于零。** 日/月/累计同时保存 priced/unpriced token、已知费用下限和覆盖
   率；100% 覆盖显示估算费用，部分覆盖显示“至少 $X · 覆盖 Y%”，完全未知显示“费用未
   提供”。任何聚合、热力图、tooltip、CSV/JSON 和 Display 协议都不得把 unknown/null
   转换成 `$0`。
7. **用户覆盖可控且可恢复。** 设置允许导入/编辑/禁用精确自定义价格，预览受影响模型和
   时间范围；错误单位、负数、通配符或无法解析的 provider/model 拒绝保存。自定义覆盖
   始终显示“用户价格”，一键恢复 ActRealm 已验证快照，不自动上传模型使用历史。
8. **更新与隐私隔离。** 价格更新请求只发送目录版本所需的通用 HTTP 元数据，不发送账户、
   prompt、项目、文件路径、session id 或本机用量。支持完全离线关闭更新；更新任务与
   Token 日志扫描解耦，不能增加首页启动延迟或长期占用 CPU/内存。
9. **ActRealm 统一定价，Display 只消费结果。** Companion 协议传递费用值、币种、
   official/estimated、price source/version、coverage、freshness 和 quality，不把完整价格
   目录复制到 Display，也不允许 Display 自己重新匹配模型或计算另一套费用。

专项回归与验收：

- 固定同一份 Token fixture，在同一 snapshot 下跨重启、离线和重复全量重建得到完全相同
  的费用；官方事件费用与本地估算同时存在时只采用官方值一次。
- 覆盖官方费用、用户精确覆盖、官方快照、第三方精确候选、未知模型、跨 Provider 同名、
  alias、长上下文阶梯、cache read/create、reasoning、批处理和币种/单位错误 fixture。
- 发布新价格版本前后，旧 ledger 的金额和 snapshot id 不变化；主动重估产生独立结果且
  能逐模型解释差额，绝不覆盖原历史。
- 断网、超时、损坏缓存、目录空响应、价格异常跳变、并发更新和 Runtime 重启均回退到
  最后已验证快照；UI 清楚显示版本与陈旧时间，没有价格时保持 unknown。
- 定价覆盖率能与 canonical ledger 逐项复算；Token 与费用热力图的合法深浅差异可以由
  模型、输入/输出、缓存和覆盖率解释。通过这些验收后，费用才允许标记为 `verified`。

### Claude 额度与重置时间专项

本机当前证据：Claude OAuth 用量能够返回 5 小时、7 天和 Extra Usage 百分比，但三个
窗口的 `resets_at` 均为 `null`。因此当前没有重置时间不是单纯 UI 漏画，而是这次官方
响应没有提供该字段。实现与验证按下列规则进行：

1. 每个额度窗口独立合并字段，来源优先级为：最新且有效的官方 StatusLine
   `rate_limits.*.resets_at` → 官方 OAuth usage 的非空 `resets_at` → 同账户、同窗口、尚未
   过期的上次已验证重置时间 → 明确标注“预计”的本机估算 → “Provider 未提供”。
2. StatusLine 的 epoch 秒和 OAuth 的 ISO 时间统一进入 UTC epoch，展示时再按本地时区
   格式化；不能直接用“当前时间 + 5 小时/7 天”伪造官方时间。
3. OAuth 返回新的百分比但重置时间为 `null` 时，不得用整份文档覆盖掉同账户、同窗口
   中尚未过期的官方 StatusLine 重置时间；改为字段级合并，并保存每个字段的来源和
   `capturedAt`。
4. 缓存不得跨 Claude 账户、额度窗口或模型范围复用；超过重置点立即失效。多个 Claude
   会话给出不同时间时取最新有效采样，不能简单取最大值，以免陈旧会话延长倒计时。
5. 默认不启用估算。若后续允许本机估算，只能基于已验证的当前窗口活动起点，并在 UI、
   API 和导出中持续标为“预计/本机推算”；跨设备使用可能造成偏差。
6. Extra Usage 或 inactive/scoped limit 没有 reset 时保持空态；不从其他窗口借用时间。
   模型级周额度只有 OAuth 或 Provider 明确提供时才显示。
7. OAuth 刷新使用共享锁、退避和自适应频率；优先吸收随正常 Claude 响应到达的
   StatusLine 数据，避免为了倒计时高频请求 Anthropic。
8. 设置增加“重置时间”“数据来源/新鲜度”“是否允许显示本机预计”三个独立开关；默认
   显示真实重置时间与来源，不默认显示预计时间。

专项测试：OAuth 非空/空 reset、StatusLine 补全、OAuth null 不擦除有效 reset、reset
过期、账户切换、多个会话陈旧数据、跨年/时区、睡眠唤醒、Runtime 重启、Claude Desktop
内嵌 Code 与独立 CLI。详细证据和竞品实现见
`docs/reports/ACTREALM_CLAUDE_QUOTA_RESET_RESEARCH_2026-08-17.md`。

性能护栏：

- 5,000 session 快照继续通过；事件到 UI p95 < 1 秒。
- hover、任务展开和筛选无 >100 ms 可感知主线程停顿。
- 空闲采集 CPU 中位目标 ≤1%；活跃采集不长期占满单核。
- 2 小时运行 RSS 不持续增长，数据库增长有界；失败时先降采集频率或按需加载。

## 8. A4 — ActRealm 诊断、恢复与用户验收门

目标：用户无需看日志，也能判断“未连接、卡住、任务不更新”发生在哪一层。

任务：

1. 诊断面板展示 Runtime 版本/协议/PID、Hook/Connector、最后 Provider 事件、snapshot
   新鲜度、Token collector、数据库 schema 和 Companion scope。
2. 将故障分成 Provider、Hook/Connector、Runtime、Companion auth、投影/UI 六层，并为每层
   提供安全恢复动作。
3. 使用当前已登录并运行的 Claude Desktop/内嵌 Claude Code 完成真实成功链路，抓取经过
   allowlist 的 StatusLine/Hook 字段并逐项比对；OAuth/StatusLine 缺字段时保留诚实空态，
   不以 fixture 或估算代替真实成功证据。
4. 进行 Runtime 崩溃、双实例、睡眠唤醒、Provider 升级、数据库迁移和断线恢复测试。
5. 打包 Apple Development 候选，验证升级和 build 36 回滚，不清除用户数据库。
6. 更新 STATUS、兼容矩阵、用户指南、已知限制和本轮验证报告。

ActRealm 用户验收门：

- 用户确认任务排序、完成隐藏、删除语义、看板信息、工作流、Token 和设置均满意。
- 未通过时只继续修 ActRealm；不得提前把不稳定规则复制到 Display。

## 9. D1 — Display Companion 与 derived 层

进入条件：ActRealm 用户验收门通过。

目标：Display 使用 ActRealm Runtime 的同一事实，同时让渲染不因高频快照重复计算而卡顿。

任务：

1. 接入 protocol v2 当前 Turn plan、timeline 分页、Attention、任务 Token、额度和
   `autoHideAt`。
2. 分成三层：wire snapshot → normalized/derived store → view state；按 session/revision
   增量更新，未变化卡片不重建。
3. derived 层只负责排序、分组、显示字段和布局优先级，不重新判断完成、风险或控制能力。
4. 保留 Companion scope：`snapshot.read`、`session.jump`、按用户授权的
   `attention.respond`；Display 不读取 ActRealm SQLite/Web Cookie。
5. 审计 ActRealm 与 Display 的旧映射；只有确认无消费者、迁移测试通过后才删除，禁止
   直接删掉仍用于兼容旧快照的字段。
6. 增加双仓库 contract fixture，防止字段、枚举、默认值和协议版本再次漂移。

验收：同一真实任务在 ActRealm/Display 的状态、当前计划、工作流、Attention、deadline
和关闭原因一致；高频 snapshot 下未变化卡片不重新渲染。

## 10. D2 — Display 自适应看板视觉定稿

目标：先用真实数据做 3 版可交互 HTML/原型，再由用户在副屏选择方向，之后才写正式 UI。

三个方向必须共享相同信息和行为，仅改变布局密度与视觉语言：

1. **Operations Desk**：成熟监控台结构，任务、注意力和流程层级最清楚。
2. **Editorial Workspace**：大字号、低噪音、适合远距离长条副屏阅读。
3. **Compact Rail**：多任务密度高，聚焦任务展开，其余任务压缩成稳定轨道。

自适应规则：

- 按容器尺寸分类 horizontal strip、compact、standard、wide、portrait，不按型号写死。
- 核心信息永远优先：状态、任务、项目、当前动作、需要处理；时间/日期和额度空间不足时
  先折叠。
- 标题使用正常产品字号，最多两行；把主要空间交给任务流程和工作流。
- 中文/英文、100/125/150% 字体、亮暗环境、Reduce Motion 和色盲状态均要可用。
- 不使用大面积 AI 渐变、无意义发光、人格气泡或占空间的装饰动画。

视觉矩阵：2880×864 长条副屏、1080p、16:10、超宽、窄竖屏、半屏、最小窗口；1/3/6
任务与 0/1/5 Attention；超长中英文任务/项目名。

## 11. D3 — Display 强交互闭环

目标：副屏平时稳定显示全部当前任务，需要人时只聚焦正确任务和正确请求。

交互：

1. 点击任务后该任务放大，显示任务流程和工作流；其他运行任务缩小但不消失。
2. 无待处理项时 Outbox 不占布局；Attention 到达时自动聚焦对应任务并打开处理区。
3. 同时多个请求严格排队；当前处理完成、失效或 handoff 后才显示下一个。
4. allow/deny 保留撤回窗口；高风险、未知风险、断线或无 reply channel 只允许返回原应用。
5. 处理动作提交后不立即宣称成功，等待 Runtime/Provider continuation 再收起。
6. 任务完成遵守 ActRealm 设置；运行超过 30 分钟不隐藏。
7. 任务允许“从 Display 隐藏/清除展示记录”，但必须解释不会停止 Provider 或删除历史。
8. 打开 Claude/Codex 时优先跳到确切会话；无法恢复历史会话时诚实降级为打开应用。
9. 工作流/计划可滚动与分页；用户阅读旧事件时不被新事件强制拉到底部。

验收：真实 6 任务、5 Attention 连续处理，无焦点循环、闪烁、队列饥饿、重复动作或任务
丢失。

## 12. D4 — Display 设置、恢复和性能

任务：

- 保留原 Agent 页面和导航；看板由该页面入口进入，Runtime/授权/显示设置回归原有位置。
- 接入字段显隐、字体密度、主题、动效、自动聚焦、完成隐藏和通知设置。
- Token 与额度分区，空间不足时使用摘要；详细图表按需打开。
- 展示 Runtime/Companion/Provider 的分层断线状态和恢复动作。
- 窗口/副屏拔插、显示器更换、缩放变化和应用重启后恢复选中任务与安全滚动位置。
- 对 derived 更新、任务卡重绘、图表和内存做 Instruments/Signpost 测量。

验收：布局切换无跳动，Display 断线不丢最后可信状态，恢复后不重放旧动作；CPU/RSS 无
持续增长。

## 13. D5 — 双端最终验收与 soak

自动化门禁：

1. Rust fmt、Clippy、workspace tests、release build、语言合同。
2. macOS UTC 全套测试与 Display 全套测试。
3. 双仓库 Companion contract、旧 schema migration、隐私 allowlist。
4. `git diff --check`、Info.plist、签名、打包和架构检查。
5. 5,000 session、timeline 分页、并发 Attention、睡眠唤醒和双实例性能测试。

真实测试：

- 7 天、至少 30 个真实任务、10 个 Attention、5 次睡眠/重启。
- Codex 与 Claude Code 都覆盖开始、计划、工具、等待、处理、继续、完成。
- Claude 额外覆盖 5 小时/7 天/scoped/Extra Usage、重置时间来源、null reset、陈旧数据和
  多会话冲突；缺失时必须显示“Provider 未提供”，不能显示假的倒计时。
- 记录等待发现时间、主动打开终端次数、误打断、重复/漏失 Attention、恢复时间、CPU、
  RSS 和数据库增长。
- 同时测试当前长条副屏和至少两种明显不同宽高比。

发布条件：零严重状态矛盾、零错误控制、零运行任务误隐藏、零敏感字段泄漏；用户确认
副屏确实减少上下文切换且没有不可接受的打断。

## 14. 提交、分支与文档策略

1. 当前 ActRealm 未提交修改继续保留；A0 体验问题修完后形成一个可回滚检查点 commit。
2. A1–A4 每阶段独立 commit，并在阶段门禁通过后 push 当前功能分支。
3. Display 从独立分支开始，不把两个仓库混在一个 commit；协议 fixture 使用明确版本。
4. 不覆盖用户已有修改，不 rebase/drop 已存在提交；整合上游时保留验证报告和迁移证据。
5. 每阶段更新：路线图状态、STATUS、协议/Schema、用户设置说明、验证报告和已知限制。
6. 未经明确授权不 tag、不建公开 Release、不部署 Cloud Functions/Rules。

## 15. 后续候选（完成双端闭环后）

- 第三个 Provider adapter：只有出现真实用户需求和稳定事件源后进入开发。
- 本地 OTLP/标准化导出：默认关闭、无内容字段，先证明实际调试价值。
- 更高级的 session/turn/tool 瀑布与异常规则：保留在 P2，不挤压任务与 Attention 核心。
- 多用户 Team：不进入 Display，本计划也不扩大现有 Team 范围。

## 16. 当前唯一下一步

继续第 7 节 A3：在 build 59 已完成 fork 去重、唯一账本、unknown/部分费用、coverage、
价格来源冻结、Models.dev 精确目录、稳健热力图、去除 K 线、合并来源/模型榜单、周期观测/
纯执行时间、可解释 suspect、整代 shadow/原子发布、真实重建状态、partial UI 复核与隐私
安全 Token 数值导出的基础上，先按 2026-08-18 用户决定完成 10 分钟 H0 短时资源门；原
2 小时与真实睡眠/唤醒结论延后到 H9 soak，不能由短门替代。A4 与用户验收完成前不修改
Display。
