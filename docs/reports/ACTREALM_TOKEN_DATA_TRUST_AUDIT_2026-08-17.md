# ActRealm Token 数据可信度审计

日期：2026-08-17

范围：本机只读检查 Runtime SQLite、Token Dashboard 展示逻辑和经过字段 allowlist 的
Codex token event。没有读取或记录 prompt、回复、完整路径、命令参数或凭据；本报告没有
修改数据库和运行代码。

> 复核说明（2026-08-17）：本报告第 1–7 节保留修复前审计证据。随后完成的 build 43
> 第一检查点已修复 inherited subagent 基线、统一三套聚合、区分费用 unknown、改用稳健
> 热力图分档，并让任务观测时间按日/月/累计变化。7 月 27 日确定性重建值为
> `58,629,794`，不再包含两次 `1,127,901,221` 继承基线。完整实施与验证记录见
> `ACTREALM_A3_TOKEN_TRUTH_CHECKPOINT_2026-08-17.md`；价格 coverage、质量状态、数值导出、
> 纯执行时间与长时资源门仍未完成，因此不能把 build 43 称为 A3 最终数据认证。

## 1. 结论

用户对当前历史数据的怀疑是成立的。至少存在两个 P0 数据真相问题和四个 P1 展示/口径
问题：

| 等级 | 问题 | 结论 |
| --- | --- | --- |
| P0 | Codex 子 Agent 继承累计基线被重复入账 | 已确认；直接制造 7 月 27 日 23 亿峰值 |
| P0 | 多套聚合总额互相不一致 | 已确认；当前没有唯一可说明的事实表 |
| P1 | 费用 unknown 被热力图当作 0 | 已确认；造成 Token/费用图案错误脱节 |
| P1 | 部分费用被当完整费用展示 | 已确认；缺少 coverage 与价格版本 |
| P1 | Agent 时间不随日/月/累计变化 | 已确认；三个周期使用同一个全历史字段 |
| P1 | 最大值线性分档被异常峰值压扁 | 已确认；其余日期大量落入最低档 |
| P1 | “同步成功”被误解为“数据可信” | 已确认；当前没有一致性审计状态 |

因此修复前的 Token 总计、峰值、部分费用和 Agent 时间不应作为精确事实对外宣称。build
41 已解决本报告确认的继承基线、聚合漂移、unknown-as-zero、周期观测时间和异常峰值分档
问题；尚未完成的 price coverage、数据质量和纯执行时间仍需继续以非最终状态展示。

## 2. 7 月 27 日 23 亿的来源

当前 UI 主数据路径 `token_usage_session_days` 对 2026-07-27 汇总为：

- 总 Token：`2,345,247,004`
- 其中两个 Codex `gpt-5.6-terra` 子 Agent 分别贡献：
  `1,131,914,533` 和 `1,131,743,560`
- 两个文件分别只有 32 和 31 条计费消息，启动时间只相差约 16 秒。

两个文件的安全 session metadata 都明确标记为 `source.subagent.thread_spawn`，拥有同一个
`parent_thread_id`。更关键的是，它们的第一条 `total_token_usage` 完全相同：
`1,127,901,221`。这不是两个子 Agent 各自在十几秒内产生的用量，而是 fork 时继承的父
任务累计基线。

现有历史 collector 在 `history_complete` 模式下把第一条 cumulative 与 0 做差，因此每个
子 Agent 都把继承基线完整记账一次。两个基线合计约 22.56 亿，解释了异常峰值。

只用于审计的交叉估算：两个文件逐轮 `last_token_usage` 合计约 840 万；把重复继承基线
替换为事件级用量后，7 月 27 日全日约为 8999 万 Token。这个数字不是最终修复值，因为还
需要验证父任务是否包含子任务用量、事件去重和历史/实时交接；正式值必须由版本化
collector 对所有原始日志做 deterministic rebuild 产生。

相同风险并不限于这一天。当前高值还包括 7 月 30 日约 8.00 亿、7 月 31 日约 11.29 亿；
需要用 subagent/fork metadata 全量审计，不能只修一个日期。

## 3. 当前存在三套不同总额

同一数据库只读汇总结果：

| 路径 | 当前总 Token |
| --- | ---: |
| `token_usage_daily` | `6,793,878,057` |
| `token_usage_session_days` | `7,423,149,465` |
| `token_usage_daily_models` | `5,691,054,870` |
| 当前 live `session_usage` | `2,856,533,291` |

这些表的职责原本不同，但 UI 会从 session-day 生成图表和总计，又从 daily 路径取得部分
记录时间/刷新元数据。某些日期 history 与 delta 相差数十到数千倍，不能仅用“渐进回补”
解释。必须建立单一 canonical ledger，其他表只能是可丢弃、可重建的 projection。

## 4. 为什么 Token 与费用热力图不匹配

### 4.1 合法的不一致

费用是 API 等价估算，不是 Pro/Max 订阅账单，也不是“每个 Token 同价”。它按以下因素
加权：

- Provider 与模型价格；
- 未缓存输入、缓存读取、缓存写入、输出的不同价格；
- 是否存在可靠的模型映射和版本化价格。

因此缓存占比高的深色 Token 日可以比输出占比高的浅色 Token 日更便宜。产品不应为了
视觉一致而强制两个热力图完全同色。

### 4.2 当前确实存在的错误

7 月 27 日共有 99.994% Token 能根据当前价格快照估算；仅 `codex-auto-review` 的
`141,817` Token 没有可靠价格。Runtime 正确地把整日完整费用标成 nil，但 macOS
`heatmapCalendar` 使用 `estimatedCostUsdMicros ?? 0`，把“未知/部分”转换成“费用为零”。
这使 23 亿 Token 的最深格在费用模式变成空白格。

同类问题还包括：

- 日/月/累计的 `optionalSum` 忽略 nil 后继续给出一个数字，没有声明这是部分金额；
- 2026-08-04 的定价覆盖只有约 12.965%，2026-08-05 约 56.710%，但 UI 没显示 coverage；
- 审计当时的嵌入价格表缺少 `claude-opus-5`，因此这些 Token 当时必须保持 unknown；
  2026-08-18 后续实现已通过 Models.dev 的精确 `claude-opus-5` ID 和相同来源的离线快照
  补齐，不是借用相近 Opus 名称或 fuzzy 匹配；
- Token 与 cost 分别使用各自全年最大值和固定 25%/50%/75% 档位。7 月 27 日异常峰值会
  把绝大多数正常 Token 日期压到最浅档，两张图也没有可比较的数值图例。

正确方向是相同日期网格、相同缺失语义、相同稳健分档方法，但保留由价格/缓存造成的
真实颜色差异；tooltip 给出 Token、估算费用、覆盖率、缓存占比和价格版本。

## 5. 为什么 Agent 时间切换周期不变化

UI 的日/月/累计切换只改变 Token、费用、活跃日、峰值、模型和消息筛选。“Agent 时间”
始终读取 `model.tokenUsage.activeTimeSeconds`。

Runtime 的该字段是所有 748 个 Turn 的全历史求和：从 `started_at` 到最后一个 event/
`ended_at`，本机当前约 99.6 小时。它没有 daily bucket，所以三个周期不可能变化。

此外这个值是 Turn 经过时间总和，不等同于 Agent 真正在执行：

- 等待用户回答或授权也可能被包含；
- 工具/外部进程等待会被包含；
- 多 Agent 并发时每个 Turn 分别相加，可能超过墙钟时间；
- 跨午夜 Turn 没有拆分到两个自然日。

修复时必须先定义“任务经过时间”“Agent 执行时间”和可选“墙钟覆盖时间”，再按自然日
切片。日/月/累计才能成为真实而不是 UI 临时换算的指标。

## 6. 其他审计发现

1. Server 会返回 `ready/scanning/partial/unavailable`，macOS 状态文案只明确处理 ready 和
   一个并不存在的 failed 分支；partial/unavailable 容易显示成“等待数据”。
2. `collectionState=ready` 只表示扫描器成功完成，没有验证 parent/child 去重、聚合恒等式、
   价格覆盖或异常跳变，不应使用“已同步”暗示可信。
3. `peakDay` 会直接选择最大总量，没有 suspect/quarantine 状态；一个已知异常即可长期
   控制峰值卡片和全年颜色尺度。
4. `recordedFrom/capturedAt` 与实际展示的 session-day 明细并不总来自同一数据路径，覆盖
   起点和最后更新时间可能无法解释屏幕上的数字。
5. 当前测试覆盖了最大格能到 level 4，但没有覆盖 inherited subagent baseline、unknown
   cost、部分 coverage、异常峰值的稳健分档或 Agent 时间周期切换。

## 7. 修复与验收决策

完整工程拆分已进入
`docs/ACTREALM_DISPLAY_NEXT_EXECUTION_PLAN_2026-08-17.md` 的 A3。顺序固定为：

1. 备份与只读审计工具；
2. Provider/version token 语义合同；
3. fork/subagent 与多来源去重；
4. canonical ledger 和 shadow rebuild；
5. 新旧逐日对账及原子切换；
6. 费用 coverage、热力图与 Agent 时间修复；
7. 全量回归、性能、回滚和真实 UI 验收。

禁止直接删除 7 月 27 日两行或把峰值手工改成 9000 万。那样无法修复 7 月 30/31 日及未来
新子 Agent，也无法证明总计正确。
