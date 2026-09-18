# ActRealm A3 Token 数据真相阶段检查点

日期：2026-08-17；最后更新：2026-08-18

分支：`agent/runtime-v2-agent-observability`
本机候选：ActRealm `0.1.0 (59)`，Apple Development 签名，Runtime schema 30

## 1. 检查点结论

build 43 已修复造成 2026-07-27 约 23 亿 Token 峰值的 Codex fork/subagent 继承累计基线，
并把 `token_usage_session_days` 收口为唯一事实账本。`token_usage_daily` 与
`token_usage_daily_models` 现在是同一事务内从事实账本重建的 projection，不再各自长期
漂移。

本检查点同时修复了费用 unknown 被显示成 `$0`、异常峰值压扁全年热力图，以及日/月/累计
共享同一任务时间的问题。真实 macOS UI、真实本机数据库和全量自动化门均已验证。

build 45 又补齐部分定价下限、定价覆盖率和可信重建状态；build 46 完成隐私安全的 Token
JSON/CSV 数值导出；build 48 完成价格种类/来源落账与历史费用冻结；build 49 完成可解释的
本机异常审计、`suspect` 状态和峰值结论排除；build 50 将“Agent 执行时间”与包含等待的
“任务观测时间”分离。build 51–57 又完成事务提交前 projection 恒等式、完整/部分历史
分离、真正的整代 shadow 发布、大日志扫描内存收口和真实 partial UI 文案复核。build 58–59
进一步删除无金融语义的 K 线、合并重复来源/模型榜单，并接入精确匹配、可离线
回退的 Models.dev 价格缓存。这不是 A3 最终完成：2 小时 RSS、数据库增长与睡眠唤醒护栏
仍需继续完成。

## 2. 修改前可恢复备份

在第一次修复写入前使用 SQLite online backup 创建：

`~/.actrealm/token-backups/a3-token-truth-2026-08-17/data-before-a3.sqlite`

- 文件权限：`0600`
- `PRAGMA integrity_check`：`ok`
- SHA-256：`1a0482e730a88acefd8acd53773c1235f6ce2415bc17341872d1eccaf2def710`
- sessions：`178`
- `token_usage_daily` 总计：`6,939,535,015`
- `token_usage_session_days` 总计：`7,590,567,265`
- `token_usage_daily_models` 总计：`5,836,711,828`

备份未删除，原始 Codex/Claude 日志也未修改。

Token 数据库备份使用独立的 `token-backups` 目录，不放入 Hook 配置备份专用的扁平
`backups` 目录；后者会安全拒绝未知目录，避免“清除配置备份”误触数据库备份。

## 3. fork/subagent 计量规则

collector 现在识别 `parent_thread_id`、`forked_from_id`、`source.subagent` 与
`thread_source=subagent`。对于继承历史的 Codex rollout：

1. session metadata 时间作为继承历史 cutoff；
2. cutoff 之前或同时的复制事件只更新 cumulative baseline；
3. 复制事件不进入日账本、不计价、不增加消息数；
4. cutoff 后只计算相对 baseline 的 session-local 增量；
5. cumulative 重置不会抹掉已经验证的历史费用。

新增 fixture `forked_codex_sessions_exclude_the_shared_parent_replay` 覆盖两个子任务共享父任务
累计值的场景。修复后的 2026-07-27 为：

- 总 Token：`58,629,794`
- 消息数：`541`
- 未定价 Token：`141,817`
- 两个异常子任务分别只保留约 `3,041,266` 与 `3,462,897` 个 session-local Token，不再
  各自计入 `1,127,901,221` 的继承基线。

修复前审计的“约 9000 万”只是基于 `last_token_usage` 的粗估，不是正式目标；build 43
使用确定性 collector 对真实历史重新计算，因此最终值允许与粗估不同。

## 4. 唯一账本与纠正路径

- 完整、已验证的历史记录可以降低或替换错误的 session-day 行；partial 数据仍保持单调，
  避免不完整扫描误删已知用量。
- 完整历史会删除同一 session 的陈旧日期/模型行。
- 每次 daily history 更新后，Runtime 在同一事务内从 `token_usage_session_days` 删除并重建
  daily 与 daily-model projection。
- collector 的内存状态是未发布的 shadow generation；历史未追平当前 EOF 时继续服务上一
  代已提交账本，不把每个扫描前缀发布成上下波动的“总计”。追平后才开始 generation，
  在一次 SQLite 事务内更新 canonical ledger、重建 projection、验证逐字段恒等式并提交；
  任一步失败会整体回滚。
- 真实回补期间多次抽查以下恒等式均成立：

  `SUM(session_days.token_total) = SUM(daily.token_total) = SUM(daily_models.token_total)`

- SQLite `PRAGMA integrity_check` 为 `ok`，178 个 session 保持不变。

历史回补是渐进的，扫描进行中时总计会继续上升；因此本报告不把某个中间时刻的全历史
总计写成最终数字。数据是否可信由恒等式、来源与完成状态判断，不由“数字不再变化几秒”
判断。

## 5. UI 与口径修复

### Token / 费用热力图

- active day 的费用缺失保持 `nil`，tooltip 显示“费用未提供”，并使用虚线紫色边框；
  只有明确的零 Token 空日才是已知 `$0`。
- Tokens 与费用共用日期网格；各自按真实数值着色，不强制颜色一致。
- 强度使用 P95 cap + `log1p`，单个极端值不再把其余日期全部压成最低档；hover 仍显示
  准确日期和原始数值。

### 价格目录与趋势图

- build 58–59 只在本机历史追平后低频读取 Models.dev 固定 API；不上传账户、
  session、Prompt、项目、路径或用量。响应有 16 MiB 上限和 10 秒超时，成功结果以
  `0600` 缓存 24 小时，失败时继续使用最后有效缓存或内置快照。
- 仅精确 provider/model ID 与已审核 alias 可进入计算；无匹配模型仍为 unknown。
  Claude 日志官方费用优先，已定价历史费用冻结；此前为 null 的同一 Token 事实允许由
  新验证目录补价。`claude-opus-5` 已通过精确 Models.dev 条目和离线快照补齐。
- partial 只描述历史覆盖，不再自动否定已观测 Token 组件的费用；输入、输出、cache
  read/create 和模型均已知时仍可给出带来源的 API 等价估算。
- Token K 线已删除：按日用量不存在金融市场的开高低收语义。总览的模型/来源重复榜单
  合并为一张“工具来源/按模型”切换卡；趋势图沿用同一切换，避免 Codex/Claude 与其主
  模型一一对应时显示两张相同图。
- build 59 实机从约 5 GB 本机来源追平后，页面由“历史重建中”切换为“历史数据部分
  可用”；随后成功写入 `3,934,279` 字节、mode `0600` 的 Models.dev 缓存。总计页为
  `4,245,518,296` Token、至少 `US$2,559.01`、定价覆盖 `90.1%`、5 个来源；canonical、
  daily 与 model 三层总计相等，SQLite integrity 为 `ok`。
- `claude-opus-5` 的 `168,267,905` Token 已全部获得精确价格，不再有未定价 Token。
  `codex-auto-review` 仍保持 unknown；`gpt-5.6-sol` 的旧 partial 记录若缺少可计费组成也
  保持 unknown，不用总 Token 反推输入/输出。损坏的新鲜缓存不再阻止重新拉取有效目录。

### 任务时间

- 文案改为“任务观测时间”。
- 语义明确为“从 Turn 开始到最后事件，包含等待”，不再冒充纯执行时间。
- Runtime 在现有 snapshot 主查询内计算今日、本月和累计观测区间，没有增加 snapshot SQL
  查询次数。
- build 50 / schema 30 新增事实表 `agent_execution_intervals`：只累计 `thinking`、
  `tool_running` 和 `compacting`，授权/问题等待会关闭区间；进程重启时按最后事实事件闭合，
  不把离线墙钟时间算成执行时间。
- 今日、本月和累计都在同一 snapshot 主查询内按本机日历边界裁剪；并发 Agent 的执行量
  按工作量相加。
- 真实 build 50 UI 验收结果：日 `11h 47m / 12h 2m`、月 `45h 42m / 65h 49m`、累计
  `77h 14m / 107h 10m`（前者为 Agent 执行时间，后者为任务观测时间），周期与语义均有
  明确差异。

### 设置

“设置 → 显示 → Token 用量”新增并验证四个独立开关：

- Token 活跃度；
- 估算 API 费用；
- Agent 执行时间；
- 任务观测时间。

旧设置解码使用向后兼容默认值，不会因新增字段把原有偏好重置。

## 6. 真实本机验收

使用 Computer Use 在 `/Applications/ActRealm.app` build 43 完成验收：

- Token 仪表板可打开，热力图 hover 显示精确日期与数值；
- 日/月/累计的任务观测时间分别变化；
- 7 月 27 日的 23 亿峰值已消失；
- 显示设置三个新开关存在且默认开启；
- Token 活跃度开关已执行关闭、保存、再恢复开启的端到端回归；
- 初次设置读取失败的“重试”按钮现在会重新读取，而不再只处理保存失败；
- Agent 任务流程和工作流继续实时更新；
- 未点击、确认或处理用户的 Outbox。

build 45 的第二次真实验收还确认：

- 总计费用按已定价部分显示“至少 US$…”，旁边显示可复算的定价覆盖率；
- 部分定价日期保留虚线质量标记，单测覆盖逐日 tooltip 的费用下限与覆盖率；
- Runtime 重启后的首次全量回补在主卡显示“首次扫描中，统计仍会增长”，Token 页显示
  “历史重建中”；
- 单个 bounded scan 成功不再被误报为“账本已验证”；只有所有已发现来源都读到当前 EOF
  才进入 verified，未追平期间没有 `lastAuditedAt`；
- build 44/45 已移动到废纸篓保留回退；该检查点当时将 `/Applications/ActRealm.app` 更新为
  build 48，现已由下述 build 49 替代。

build 46 继续验证：

- “设置 → 数据”新增“导出 Token JSON…”与“导出 Token CSV…”及明确隐私说明；
- 两个按钮都通过真实本机 Runtime 请求并打开对应保存面板，测试随后取消，未写下载目录；
- JSON `scope=token_usage_numeric`，包含口径、总计、逐日 Provider/模型行和数据质量；CSV
  使用同一事实行并保留 priced/unpriced 与费用下限；
- 自动化 fixture 证明导出不包含两个测试 session ID，也没有 sessionId、prompt、path、
  command、toolContent 或 response 字段。

build 48 继续验证：

- Runtime schema 29 为 `token_usage_session_days` 增加 `cost_kind` 与 `pricing_source`；升级前
  费用标成 `legacy_unclassified / legacy_pre_v29`，没有伪造其历史价格来源；
- Token 事实未变时，新价表计算结果不会覆盖旧费用；Provider 日志的官方费用可以升级计算
  结果；Token 事实或模型被修正时允许同步替换费用；
- 总计快照、JSON、CSV 与 Swift model 均携带来源；真实 UI 显示“3 个来源 / 历史费用按写入
  来源冻结”，且仍明确显示“历史重建中”；
- 真实数据库升级 integrity 为 `ok`，schema 为 29；零 Token/零费用的 Claude 行不会计入
  来源数量；
- schema 29 写入前的额外 online backup 位于
  `~/.actrealm/token-backups/a3-price-freeze-build47-2026-08-17/data-before-schema29.sqlite`，
  SHA-256 为 `63b0b3cbaa20b201c7066223183c3068a98a693eefb546b61f11de661af1f930`；
- build 46 与 build 47 已移动到废纸篓保留恢复路径，没有清空废纸篓。

build 49 继续验证：

- Runtime 在现有快照读取中审计 canonical ledger 与 daily/model projection 的总和一致性，
  并检查未来日期、持久化负数和有足够活跃日样本时的极端单日跳变；没有增加主快照的 SQL
  查询次数；
- 只有可证明会影响 Token 结论的问题才进入 `suspect`。价格覆盖不足是 warning，Token 总量
  仍可用，费用继续只显示已定价下限；
- `suspect` 日期默认不参与“峰值日”结论；原始逐日数值仍保留在图表和数值导出中，没有
  静默删改历史；
- 快照和 Token JSON 导出携带 bounded 的异常 code、severity、scope、日期及
  observed/expected 数值，不包含 Prompt、路径或命令；
- 真实 build 49 Token 页显示可展开的“费用覆盖不完整，Token 总量仍可用”，展开后显示缺少
  可靠模型价格的 Token 数；真实本机账本未发现 suspect，价格 warning 没有错误地把整体
  data quality 降为 suspect；
- build 48 已移动到
  `~/.Trash/ActRealm-build48-a3-before-anomaly-audit.app` 保留回退，没有清空废纸篓。

build 50 继续验证：

- Runtime schema 30 新增可审计的执行区间；真实历史保守回建为 892 个区间，迁移后只有
  当前运行任务保留 1 个开放区间，且不存在同一 session 多个开放区间；SQLite integrity
  为 `ok`；
- 授权等待 fixture 验证 70 秒观测区间只计 30 秒执行时间；跨本机日/月边界 fixture 验证
  日、月、累计分别裁剪；
- schema 30 写入前已创建 mode `0600` 的 SQLite online backup：
  `~/.actrealm/token-backups/a3-execution-time-build49-2026-08-17/data-before-schema30.sqlite`，
  integrity `ok`，schema 29，179 个 sessions、17,857 个 events、事实账本总计
  `3,703,425,086` Token，SHA-256
  `f73aaf602797e33f8e7ce54c0714605414490c120d538c832b909dd919b4bc22`；
- 真实 Token 页验证日/月/总计切换与两种时间并列展示；“设置 → 显示”验证独立开关和完整
  口径说明。验收未处理用户 Outbox；
- build 49 已移动到
  `~/.Trash/ActRealm-build49-a3-before-execution-time.app` 保留回退，没有清空废纸篓。

build 51–57 继续验证：

- canonical ledger 写入、daily/model projection 重建和提交前逐字段 invariant 已纳入同一
  SQLite 事务；故障注入证明不合法 generation 会整代回滚，重建后可再次通过；
- `is_caught_up` 与 `is_history_complete` 已分成两个事实。本机存在 1 个超过 1 GiB 上限的
  Codex rollout，因此追平 EOF 后诚实显示 `partial`，不生成 `lastAuditedAt`，也不把部分
  历史叫作 verified；
- Codex JSONL 使用顶层事件类型预筛选，只有 `session_meta`、`turn_context` 和
  `event_msg/token_count` 才完整反序列化；单行内存上限为 2 MiB、每轮吞吐仍为 10 MiB，
  并由 collector 跨文件、跨刷新复用同一缓冲；
- build 53 的真实扫描曾达到 `115,296 KiB` Runtime RSS，明确判失败；build 55 的同一真实
  数据扫描峰值降到 `23,856 KiB`。build 56 最终验收前 6 分 20 秒峰值为 `25,296 KiB`，
  SwiftUI 外壳启动峰值约 `98 MiB`、随后约 `67–76 MiB`，两者分开计量；
- build 56 启动前冻结的已提交总计为 `4,425,062,023`。历史回扫期间三层总计保持完全不变，
  20:19:59 追平后一次切换到 `4,438,400,572`；随后约 15 万 Token 的变化来自本对话仍在
  运行产生的实时增量，不是历史前缀抖动；所有采样继续满足三层求和恒等式；
- build 57 将追平后的稳定 partial 状态统一显示为“历史数据部分可用”；真实主卡片与 Token
  仪表板均已复核，扫描期间仍显示“首次扫描中，统计仍会增长”。验收未处理用户 Outbox；
- build 57 的 8 分钟实机门覆盖 48 个采样点：canonical/daily/model mismatch 为 0，Runtime
  RSS 峰值 `23,936 KiB`；SwiftUI 进程在打开完整 Token 面板和无障碍树后从 `190,160 KiB`
  回落到 `134,608 KiB`，短时未见持续增长。该结果不替代 2 小时门；
- build 50–58 均已逐版移动到废纸篓保留回退，没有清空废纸篓；当前安装的是 build 59，
  build 58 位于 `~/.Trash/ActRealm-build58-before-build59.app`。

## 7. 自动化与构建门

已通过：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `cargo build --workspace --release --offline`
- `cargo test --workspace --offline`
- Rust usage 单测 28 项，其中包含 inherited subagent fixture、EOF/完整性门槛、Codex
  两阶段解析路由、Models.dev 精确目录和 partial 已知组件补价
- Rust server 单测 47 项，覆盖“成功但未追平不能标 verified”、未来日期 suspect，以及
  未完成 shadow scan 不得修改上一代账本
- macOS Swift 全量测试 183 项 / 26 suites
- `scripts/check-runtime-language.sh`
- `git diff --check`
- arm64 Apple Development 深度签名验证
- 最终源码独立 120 秒资源门：117 个样本，平均空闲 CPU `0.009%`、Runtime RSS 峰值
  `7,888 KiB`，低于 `0.5% / 81,920 KiB` 门槛

安装包：

`apps/macos/dist-a3-build59/ActRealm.app`

本机旧候选已移动到废纸篓保留恢复路径，没有清空废纸篓。

## 8. 仍未完成的 A3 阻塞项

2026-08-18 用户将当前 H0 资源门改为十分钟。短门的 118 个真实负载样本已通过，Runtime
RSS 峰值 `23,232 KiB`，三层 mismatch 为 0，受控重启恢复通过；该结果不替代原两小时或
真实物理睡眠/唤醒结论，长期门转入 H9。证据见
`docs/reports/ACTREALM_H0_SHORT_SOAK_2026-08-18.md`。

1. H0 十分钟门已经通过；原两小时、真实睡眠/唤醒和更长期数据库增长结论作为 H9 发布前
   门保留。首次全量回补、整代 shadow 切换和 120 秒空闲 CPU/RSS 门也已通过。
2. 当前进入 H1/H2；用户确认 ActRealm 满意前不修改 Display。
