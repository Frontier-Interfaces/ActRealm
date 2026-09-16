# ActRealm H4 Token 决策信息检查点

日期：2026-08-18

分支：`agent/h4-h5-control-plane`

候选：ActRealm `0.1.0 (67)`，Runtime SQLite schema 33，Apple Development 签名

结论：H4 通过。真实 build 66 UI 已验证归因、未分配、项目/任务列表和燃烧速度空态；验收中
发现一条 Provider UUID 被旧 session.project 当作项目名，build 67 已将 UUID/unknown/untitled
降级为项目未分配。新增信息只回答“Token 归属、当前速度、
是否超过用户本机阈值”，没有把本地 Token 换算成官方套餐额度，也没有把 API 等价费用称为
订阅账单。

## 1. 任务与项目归因

- 总账继续以本机事实 ledger 为准；
- 通过 Provider + Provider session identity 与 Runtime session 明确连接；
- 有明确 session 的 Token 计为已归因；无法连接的历史/孤立记录保留为未分配；
- 展示归因覆盖率、未分配数量、前20个项目和前20个任务；
- 项目/任务合计不反向改写总账，覆盖率可由三个总数复算；
- 任务标题沿用 Runtime 已有安全标题，不加入 cwd、仓库 URL 或完整路径。
- UUID、unknown、untitled 和控制字符项目标签不进入项目排行，避免把内部标识展示为项目名。

## 2. 真实燃烧速度

schema 33 新增本地私有 `token_usage_rate_samples`：

- 只为当前仍在运行的真实 Turn 采样；
- Provider 用量事实必须不早于当前 Turn；
- 当前窗口为5分钟，基线窗口为此前30分钟；
- 首个样本只建立基线，不产生速度或提醒；
- 下降的累计计数视为 compaction/reset，绝不当成负用量；
- 未变化记录最多30秒保存一次，保留6小时；
- 同一任务当前速度达到30分钟基线3倍且窗口增加至少10K时只标记`elevated`，不写“浪费”；
- 计算结果使用2秒只读缓存，不把额外查询加到 Hook 写入路径。

## 3. 阈值通知与设置

新增本机设置：

- 任务/项目归因显示；
- 当前燃烧速度显示；
- 用量异常与数据质量显示；
- 燃烧速度通知总开关；
- 50K、100K、250K、500K、1M Token/分钟阈值。

通知只来自至少两个实时样本形成的当前窗口；首次基线和历史回补不会触发。同一个Turn在本机
7天去重一次。点击通知激活ActRealm并打开Token仪表板。模块均可关闭，关闭展示不删除事实
账本。

## 4. 三种信息严格分区

- 官方额度仍只来自Codex/Claude官方或已验证桥接窗口；
- Token归因与燃烧速度标记为本机事实账本；
- 费用继续显示价格来源、覆盖率和“API等价估算”语义；
- 本地Token永远不推断用户是5x或20x，不映射官方额度百分比。

## 5. 客户端与隐私

- macOS Token仪表板新增归因、项目/任务和燃烧速度卡；
- macOS设置接入全部新增开关和阈值；
- Web额度栏显示安全的紧凑摘要，并在保存设置时保留完整合同；
- `tokenDecision` 不进入 Companion snapshot，因此 Display/Watch 当前不会获得任务/项目历史；
- 没有新增 Cloud payload、Prompt、完整命令、文件内容或路径。

## 6. 自动化证据

- Runtime：归因覆盖、未分配、首次基线、真实速率、阈值与路径不泄露；
- Runtime：速率必须有两个单调样本和至少5秒真实间隔；
- Server：本地 snapshot 包含 H4，Companion 明确不包含；
- macOS：旧 snapshot/旧设置保持安全默认，新合同可编码/解码；
- Web：原通知声音移至独立模块，`app.js` 仍低于128KiB预算；
- 完整 workspace、release、语言、macOS 和真实性/性能门在提交前执行。

## 7. schema 33 备份

升级前 SQLite online backup：

`~/.actrealm/token-backups/h4-token-decision-schema33-2026-08-18/data-before-schema33.sqlite`

- mode `0600`，目录 mode `0700`；
- user version 32；integrity `ok`；
- SHA-256：`22666e08a68e4023da05fd89847f1c5cebcba8648077483788a0b36baa3b7589`。

## 8. H4 验收边界

H4 通过不表示本地速度等于 Provider 官方计费速度，也不表示官方套餐剩余额度。真实 UI 已
确认归因覆盖率可读、未分配值突出、阈值关闭时无通知、首次历史重建无通知且仪表板没有横向
溢出。燃烧速度需要后续真实任务继续积累5分钟窗口，但首次空态和自动采样行为已通过。
