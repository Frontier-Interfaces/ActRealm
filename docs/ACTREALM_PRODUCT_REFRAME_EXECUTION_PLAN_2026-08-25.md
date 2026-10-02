# ActRealm 产品重构完整问题清单与执行计划

日期：2026-08-25

分支：`actrealm最新版`

当前安装代码基线：`d6c891f5548518063e2fac38b00f4dfb57922d6e`

当前安装版本：ActRealm `0.1.0 (88)`，对应提交 `d6c891f`；R0–R4 工程修改均已 commit、push、
安装并完成自动化、真实审批、Computer Use、Token 生命周期和短时资源门。

状态：**R0–R4 候选门已通过；仍需 20 个新的 post-reframe Review 任务、VoiceOver/键盘人工门、
7 日 soak 和用户最终验收。H6、Watch、Claude Cowork、H8 Display 继续暂停。**

## 1. 产品重新定义

ActRealm 不再以“尽可能显示 Agent 的所有信息”为目标。默认界面只回答四个问题：

1. 现在谁真正需要用户处理？
2. 仍在运行的 Agent 正在做什么？
3. 任务结束后实际修改了什么、验证了什么？
4. 用户如何返回、继续、拒绝、归档或安全恢复？

Token 分析、完整历史、诊断、Checkpoint、Team、移动端和 Companion 都是二级能力，不能与
上述四个问题争夺首页注意力。

## 2. 状态定义

| 状态 | 含义 |
| --- | --- |
| `DONE-B88` | 已进入 build 88，完成 commit/push/install 和对应工程门；不等于用户最终验收 |
| `PARTIAL` | 已有一部分真实能力或本轮已收敛一部分，但仍有明确缺口 |
| `TODO` | 尚未修复，进入后续阶段 |
| `PARK` | 明确暂停，不属于当前 ActRealm 本地控制面交付 |

当前 32 项审查问题中：`DONE-B88` 18 项、`PARTIAL` 12 项、`TODO` 1 项、`PARK` 1 项。
`DONE-B88` 代表已进入当前安装候选并通过对应工程门，不代表用户已经满意。

## 3. 完整问题清单

### 3.1 审批与安全

| ID | 审查发现 | 风险/用户影响 | 修复要求 | 当前状态 |
| --- | --- | --- | --- | --- |
| S1 | `git status ; rm file` 等复合命令可能先命中 Git 只读规则并返回低风险 | 用户可能在错误的“低风险”提示下批准含副作用命令 | 组合语法必须在任何低/中风险返回前 fail closed；已知高影响规则仍优先 | `DONE-B88` |
| S2 | 风险判断仍是字符串/Token 规则，不是 Shell AST、系统权限或目标范围分析 | alias、函数、`bash -c`、`python -c`、编码脚本、云 CLI、数据库迁移等无法准确解释 | 建立对抗命令矩阵；无法证明时保持 unknown，不宣传“安全检测” | `TODO` |
| S3 | 主 OUTBOX、HUD、菜单栏和远程投影曾执行不同批准策略 | 同一请求从不同入口处理结果不同 | 所有入口消费同一 `canApproveFromRedactedSurface` 规则 | `DONE-B88` |
| S4 | 本地主审批卡曾不显示风险等级和原因，同时提供直接允许与二次确认允许 | 风险功能存在但没有真正参与主交互 | 主卡显示风险/理由；证据不足时只显示拒绝与回原窗口 | `DONE-B88` |
| S5 | `rm <redacted>`、`git <redacted>` 无法说明目标、参数、branch、remote 或范围 | 脱敏后信息不足以安全批准 | 只有精确 `git status`、`git diff`、`git log` 保留可批准语义；其他 shape deny/hand-off；完整范围留在 Provider | `DONE-B88`（保守边界）；更丰富的安全目标摘要不实施，除非先定义不泄密合同 |
| S6 | 中风险文件编辑、安装、网络和进程操作曾允许远程 approve | 可能运行生命周期脚本、覆盖文件或产生外部副作用 | medium/high/unknown 和隐藏目标一律远程 deny-only | `DONE-B88` |
| S7 | “3 秒可撤回”容易被理解为命令执行后也能撤销 | 形成错误安全感 | 所有文案改为“决定尚未提交，可撤回”；提交后明确不可撤销实际副作用 | `DONE-B88` |

### 3.2 首页信息架构与视觉层级

| ID | 审查发现 | 风险/用户影响 | 修复要求 | 当前状态 |
| --- | --- | --- | --- | --- |
| I1 | OUTBOX 为空时仍占据约四分之一到三分之一窗口 | 活跃任务被无意义压缩 | 无待处理和无撤回决定时完全收起；有新事项时稳定展开 | `DONE-B88` |
| I2 | 首页同时承担审批、任务、Token、额度、团队和运行状态 | 用户无法快速判断下一步 | 首页只保留 Attention、活跃任务、紧凑官方额度；其他能力进入二级入口 | `PARTIAL`：空 OUTBOX 与 provisional Token 已收敛，模块仍需继续减量 |
| I3 | 智能聚焦、历史、加入、设置及连接提示同时常驻顶部 | 非核心入口稀释主任务 | 顶部只保留工作区、历史、设置；Join/Team/实验能力迁入设置或 Labs | `PARTIAL`：Join 已迁入设置；Agent Focus 仍作为 Attention 快捷入口保留待用户判断 |
| I4 | 展开任务同时塞入上下文、Token、价格、Review、Plan、活动和恢复 | 信息密度高，缺少明确结论 | 顶部先显示“是否需要我/当前动作”；完成后显示 Review 结论；其余折叠 | `PARTIAL`：运行中先显示计划/重要活动，核心事实单独展示，Token/开发者字段进入“更多详情” |
| I5 | 玻璃背景、小号灰字、密集表格和低对比度辅助文本 | 长时间阅读困难，副屏或远距阅读更明显 | 建立最小字号、对比度、行高和点击目标护栏；浅/深色分别验收 | `PARTIAL`：设计系统默认字号已上调；VoiceOver/对比度人工门未完成 |
| I6 | 现有布局仍有 1160pt 最小宽度和多处固定列宽 | 换屏或窄窗口时适应性有限 | 建立窄/中/宽三档布局；模块按优先级重排，不仅缩放 | `DONE-B88`：最小宽度降至 900pt；窄窗 Attention 使用任务主列 + OUTBOX/额度堆叠侧栏 |

### 3.3 Token、额度、价格与性能

| ID | 审查发现 | 风险/用户影响 | 修复要求 | 当前状态 |
| --- | --- | --- | --- | --- |
| T1 | `partial/rebuilding` 数据仍在任务卡和首页显示十亿级 Token 与数百美元 | 不完整数据被误认为权威结论 | scanning/partial/unavailable/suspect 时首页只显示核对状态；价格隐藏 | `DONE-B88` |
| T2 | Token 仪表板虽有质量 badge，巨大累计值仍是视觉中心 | 用户容易忽略数据覆盖状态 | 详细页所有总计统一加“已观测/完整/部分”口径；suspect 时禁止峰值和排名结论 | `DONE-B88`：未验证账本暂停峰值、排行、归因、热力图和趋势 |
| T3 | Codex fork/subagent 日志可能重放父会话；公开项目曾出现 91 倍虚增 | 历史用量和价格可能严重偏大 | 用当前真实日志做 shadow rebuild，验证 fork 边界、重复 token_count 和旧 frozen 行失效 | `PARTIAL`：已独立抽查当前日志与 canonical 边界，证实 partial 数据不足以支持峰值/排行；完整回补完成态仍待观察 |
| T4 | API 等价价格容易被 Pro/Max 用户理解成真实支出 | 与订阅账单、额度消耗混淆 | 只称“API 等价值”；默认不进任务卡；本地 Token 与官方额度永远分区 | `DONE-B88`：全局改名并明确“非订阅账单”；provisional 时隐藏 |
| T5 | 趋势、热力图、模型/Provider 排名存在展示价值，但与下一步决策关联弱 | 增加复杂度和刷新成本 | 只保留能支持阈值、异常和项目决策的图表；删除重复排名和装饰图 | `PARTIAL`：未验证历史时全部暂停；验证完成后的最终精简仍待用户评估 |
| T6 | 审查期间 UI/Runtime CPU 在活跃长会话与重建状态下明显偏高 | 看板反过来影响 Agent 和系统流畅度 | 等值状态不发布；刷新后台化/合并；完成 idle、双 Agent、重建、长会话资源门 | `PARTIAL`：build 88 短时实机门通过，空闲 Native 多次为 0–4%；首次 2.8GiB 回补仍为 3–8%，双 Agent 与 7 日 soak 未完成 |

### 3.4 任务流程、最近活动与 Review

| ID | 审查发现 | 风险/用户影响 | 修复要求 | 当前状态 |
| --- | --- | --- | --- | --- |
| A1 | “工作流”实际只是脱敏工具事件时间线 | 名称过度承诺 | 改名“最近活动”，不再暗示完整因果工作流 | `DONE-B88` |
| A2 | 活动中仍可能出现大量 Bash/MCP、重复成功和“未收到结束事件” | 用户被调试噪音淹没 | 默认只显示最近 3–5 个重要事件；合并短成功；失败、验证、等待用户保持独立；完整时间线按需加载 | `DONE-B88`：默认最多 5 个重要/最新事件，用户可展开本轮全部并继续分页 |
| A3 | Plan 必须依赖 Provider 结构化步骤；曾出现 Provider 第 1 步而 ActRealm 0 步 | 任务进度失去信任 | 只显示真实结构化步骤和来源；无事件时明确缺失；20 个真实任务验证零错误步骤 | `PARTIAL`：合同和清理逻辑已有；新的 20 任务实机矩阵未完成 |
| A4 | Review 在运行任务、无 Git、无验证时仍常驻并显示空结论 | 占空间但不能指导下一步 | 运行中只显示有证据的摘要；完成时突出修改、验证、失败和未验证 | `PARTIAL`：运行中空 Review 已隐藏、证据结论已重排；仍需 20 个真实任务验证零误报 |
| A5 | 已提交改动、dirty baseline、嵌套仓库和并发任务可能导致“0 修改”难以解释 | 用户不知道 Review 是当前 diff 还是任务归属证据 | 明确 baseline、attribution、commit 和 working-tree 口径；返回“无当前 diff”不能等同“任务未修改” | `DONE-B88`：0 diff 改为“无未提交变更”，并提示结合 Commit/历史 |

### 3.5 历史、跳转与 Checkpoint

| ID | 审查发现 | 风险/用户影响 | 修复要求 | 当前状态 |
| --- | --- | --- | --- | --- |
| H1 | 历史列表混入大量 H2.4、smoke 和并发验收任务 | 历史更像 QA 数据库而不是用户成果 | 明确内部验收标记/数据域；当前先严格规则默认隐藏并允许手动显示 | `DONE-B88`；长期应由 Runtime 写入正式来源字段替代标题规则 |
| H2 | 无 Git、无验证、无 Checkpoint、无安全事件仍显示三张空卡 | 空界面噪音 | 合并为一条“无可验证证据”，只有存在内容时再展开对应卡 | `DONE-B88` |
| H3 | “返回原会话”有时实际只能打开应用 | 按钮过度承诺 | 按 exact conversation、terminal、app only、unsupported 显示真实动作 | `DONE-B88` |
| H4 | Checkpoint 默认可能只有本机元数据，Provider 恢复常不支持 | 功能名称容易让用户以为一定可恢复 | 只在有 Git snapshot 或可验证 resume 时突出；metadata checkpoint 进入高级入口 | `PARTIAL`：运行中无 Review 证据时不再突出 metadata-only Checkpoint；完整产品语义待用户判断 |

### 3.6 诊断、实验能力和产品边界

| ID | 审查发现 | 风险/用户影响 | 修复要求 | 当前状态 |
| --- | --- | --- | --- | --- |
| D1 | 分层诊断真实但过密、术语多、字号小 | 普通用户难以使用 | 默认只回答“哪层有问题/下一步做什么”；完整技术字段折叠到支持详情 | `DONE-B88`：默认只显示六层结论，版本/实例/Provider/暂停功能折叠 |
| D2 | Join、Team、Cloud 在个人控制面中占据高层入口 | 核心定位模糊 | 保留底层能力，但迁入 Collaboration/Labs；默认工作区不展示 Team | `PARTIAL`：Join 已移入设置；Team Today 与完整 Team 设置仍待最终边界决定 |
| D3 | iPhone、Watch、Claude Cowork 和 H8 Display 尚未完整验收 | 未完成功能增加心理负担并可能复制错误规则 | 继续 `PARK`；只在 Labs/路线图中显示，不作为当前故障 | `PARK` |
| D4 | Agent Focus/HUD 能减少切换，但会放大错误 Attention 或审批策略 | 错误自动聚焦比不聚焦更打扰 | 审批策略先统一；随后验证队列、去重、返回和不抢焦点 | `PARTIAL`：批准动作已统一；干扰度和多任务实机矩阵未完成 |

## 4. 本轮实际完成进度

本轮对应 `S1/S3/S4/S5/S6/I1/T1/A1/H1/H2/H3`，并推进
`I2/I6/T4/T6/A2/A4/D4`：

1. Runtime 调整审批分类顺序并增加复合命令对抗测试。
2. 新增 `canApproveFromRedactedSurface`：只有 exact low-risk Git read 可以看到 Allow。
3. 主 OUTBOX、HUD、菜单栏、共享任务、个人远程 envelope 与 Firebase server 使用同一规则。
4. medium/high/unknown、隐藏目标和未知 shape 均为 deny-only/回原窗口。
5. 远程 shape 可保留 `git status/diff/log`；其它仍为 `<redacted>` 或 unknown。
6. 空 OUTBOX 自适应收起；pending undo 时仍保留。
7. Token provisional 状态不再在首页和折叠任务卡展示累计/价格；展开后只显示“已观测、未完成核对”。
8. `AppModel` 不再发布相同 Token、Token decision、阈值提示和 DerivedState。
9. “工作流”改为“最近活动”。
10. 历史默认隐藏明确内部验收任务，合并空证据卡并修正跳转文案。
11. 中英文资源、共享 schema、Cloud validation 和文档同步更新。

### 4.1 已通过门

- Rust fmt、Clippy `-D warnings`、全 workspace tests、release build：通过；
- Firebase Functions TypeScript build 与 42 tests：通过；
- macOS 最终门：35 XCTest + 201 Swift Testing：通过；
- language/runtime contracts、Info.plist、JSON schema、`git diff --check`：通过；
- SnapshotTool 生成 28 张中文编译截图并检查主窗口、展开任务和菜单栏；
- 高风险 demo 只显示“拒绝/去原窗口核对”；provisional Token 首页不再显示数值。

### 4.2 候选固化后仍未发生

- 没有执行危险命令；合成 Hook 请求验证了策略，但这不是任意 Shell 安全证明；
- 尚未完成 20 个新的 Codex/Claude Review 任务；
- 尚未完成 VoiceOver/键盘人工门和 7 日资源 soak；
- 首次 Token 回补期间的资源已测，回补完成后的长期空闲资源仍需记录；
- 用户尚未最终验收本轮信息架构和交互；
- H6/Watch/Cowork/H8 Display 没有恢复。

## 5. 后续执行阶段

### R0 — 候选固化与安全实机门（build 82 已通过）

前置：用户明确授权 candidate commit/push。

1. commit 并 push `actrealm最新版`，保存精确 SHA；
2. 构建 Apple Development 签名 build 82，嵌入该 SHA；
3. 保留 build 81 可恢复副本，再安装 build 82；
4. Doctor、codesign、Runtime commit/schema/Companion protocol 核对；
5. 真实审批矩阵：
   - `git status`、`git diff`、`git log`：可允许/拒绝，3 秒决定撤回；
   - `apply_patch`、package install、network、process、hidden-target read：无直接 Allow；
   - `rm -rf`、git push、credentials：高风险且无直接 Allow；
   - `git status ; rm file`：unknown/compound，无直接 Allow；
   - 主窗口、HUD、菜单栏、共享/远程动作完全一致；
6. Computer Use 检查空 OUTBOX 展开/收起、焦点、风险文案和任务空间稳定性。

验收：零 false-low、零入口策略差异、零过期请求可操作。

### R1 — 首页信息架构第二轮（build 88 已安装）

1. 顶部导航收敛为工作区、历史、设置；Join/Team/实验能力迁入设置/Labs；
2. 活跃卡默认只显示任务、项目、Agent、模型、当前动作、状态、运行时间和真实计划摘要；
3. 展开顺序固定为“需要处理 → 当前事实 → Review 证据 → 最近活动 → 高级详情”；
4. Review、Checkpoint、Token detail 没有证据时不占常驻空间；
5. 1160/1440/宽屏和浅/深色实机验收。

验收：用户在 3 秒内能指出哪个任务需要处理、下一步是什么。

### R2 — Token 真相与资源收口（`PARTIAL`，短时资源门通过、完整回补与 7 日门待测）

1. 对当前真实数据库执行可回滚 shadow rebuild；
2. 从原始 Codex/Claude 文件抽样复算 parent、fork/subagent、duplicate token_count、cache；
3. 给旧 frozen 行增加 parser/source 版本失效策略，修复后自动重建；
4. Token dashboard 将“已观测、部分、完整、可疑”放到数字同一视觉层级；
5. API 等价值从任务卡完全移除，只在 Usage 详情提供；
6. 删除重复/无决策价值排行；
7. idle、双 Agent、首次重建、长会话分别 profile；后台窗口停止不必要渲染。

验收：三层账本一致只是必要条件；还必须能由原始文件独立复算且解释差异。

### R3 — Review 与最近活动成为真正验收闭环（`PARTIAL`，20 任务门待测）

1. 完成态先回答 changed / validated / failed / not run / unverifiable；
2. 明确 current diff、task-attributed change、commit 和 baseline 的区别；
3. 最近活动默认 3–5 项重要事件，完整时间线按需加载；
4. Codex/Claude 各至少 10 个真实任务，覆盖修改、无修改、测试通过、失败、未运行、嵌套仓库和并发；
5. 不允许一次虚假的“测试通过”。

验收：20 个任务中用户无需先翻终端即可决定下一步，且证据零误报。

### R4 — 历史、Checkpoint 与支持体验（`PARTIAL`，无障碍与产品边界待验收）

1. Runtime 增加正式 `origin=user|validation|internal` 字段，替代标题启发式隐藏；
2. 历史默认围绕成果、验证和恢复组织；
3. Metadata Checkpoint 降级为高级信息，Git snapshot/resume 可用时才突出；
4. 诊断首页只给层级结论和恢复动作，技术详情默认折叠；
5. 完成键盘、VoiceOver、对比度、字号、滚动和焦点顺序验收。

### R5 — 产品验收与 Display 决策

1. 完成 7 日稳定性/资源 soak；
2. 用户逐项验收 Attention、任务、Review、活动、历史、Token、额度和诊断；
3. 仍不满意时继续只改 ActRealm；
4. 只有用户明确确认 ActRealm 信息和交互后，才创建 H8 Display 实施候选；
5. H6 mobile、Watch 和 Cowork 仍需独立重新授权，不能随 Display 自动恢复。

## 6. 禁止继续堆叠的内容

- 首页原始 Bash/工具日志；
- AI 推算的虚假进度、剩余时间或测试结论；
- Token K 线、重复模型/Provider 排名和装饰性指标；
- 默认自动批准、YOLO 或高/中/未知风险远程 approve；
- 所有历史任务重新进入活跃看板；
- 在 ActRealm 未验收前复制规则到 Display；
- 把 Langfuse/Phoenix、IDE、终端或完整 Kanban 搬进 ActRealm；
- 为了展示已有代码而把未完成 mobile/Watch/Cowork 放到默认入口。

## 7. 市场参照及反面证据

- GitHub Copilot Agents：状态、日志、引导、停止、归档、Commit 与会话证据关联；
- Cursor Background Agents：状态、追问、搜索和接管；
- Entire：Session/Checkpoint 与 Git 状态绑定；
- CodexBar：来源、stale/error 和自适应后台刷新；
- ccusage：Codex fork/subagent 历史重放导致 Token 最高 91 倍虚增的真实问题；
- Vibe Kanban：状态跳变、队列卡住、长 Diff 卡顿和历史日志 OOM 是需要避免的反面案例；
- Langfuse：Trace/Graph 适合生产可观测性，不应成为本地人类控制面的默认复杂度。

这些产品用于验证行为模式，不用于逐像素复制，也不以 GitHub Stars 代替用户需求证据。

## 8. 当前下一动作

当前下一动作是：**使用 build 88 完成 20 个新的 Codex/Claude Review 任务、VoiceOver/键盘顺序、
首次 Token 回补完成状态和 7 日 soak，并由用户判断 Agent Focus、Team Today、Checkpoint 与完整
Token 图表是否仍值得保留。** 仍不恢复 Display 或移动端工作。
