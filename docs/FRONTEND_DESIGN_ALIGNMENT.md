# ActRealm × Claude Design 前端对齐说明

状态：ActRealm 主界面对齐已落地；2026-07-18 单标题栏精简已通过本机视觉验收。基于设计画板 6/7 的首次进入与统一接入中心于 2026-07-20 通过本机视觉验收，真实 Provider 功能验收及 commit/push 仍待完成。

设计基准：`ActRealm Interactive Demo04.dc.html`。可见界面与可见功能以该设计为准；ActRealm 已有后端能力不删除，设计没有入口的能力先隐藏，后续可重新接回。

## 本轮可见范围

### ActRealm 工作区

- 使用单一 ActRealm 标题栏：不再模拟 macOS 系统菜单栏，也不显示装饰性的红黄绿窗口圆点；品牌位于左侧，“通知与数据”、本机时间和真实 Runtime 状态位于右侧。
- `OUTBOX / AGENT TASKS / QUOTA` 三栏比例、玻璃材质、颜色、密度和交互层级对齐设计。
- OUTBOX 支持批准、拒绝、二次确认后允许、问题回答、完成确认、返回原 Agent 和三秒撤回。
- Provider 原生审批与 ActRealm 可回复审批严格分开：原生审批只允许返回原界面，不显示虚假的批准/拒绝按钮。
- 任务卡显示 Provider 真图标、官方会话标题/任务内容、模型、实时状态、总耗时、计划和子 Agent；点击展开安全详情。
- 清除任务会安全交还关联中的阻塞请求并隐藏任务；收到更新事件后任务重新出现，不删除历史数据。
- 额度按 Provider 实际返回的任意窗口动态渲染，不写死“本周”；不可用和过期数据不会伪造成实时额度。

### 通知与数据

- Runtime/`bridge.sock` 状态、监控折叠区和安全通道重启。
- “什么时候提醒”每一类只有“不提醒”和“仅列表”。
- 提示音开关。
- 事件保留支持 30 天、90 天、180 天和永久；永久在存储层表示不自动清理。
- 导出本机事件、导出聚合统计和输入 `DELETE` 后彻底清除。
- 活跃天数、面板决定数、处理率、交还率、平均响应和页面渲染 p95。

### 首次进入与统一接入中心（画板 6/7）

- 两个 Provider 都未安装时，默认进入三栏空主界面；OUTBOX、Agent Tasks、Quota
  使用同一份真实 `firstRun` 状态，不短暂闪现历史数据或伪造额度。
- 标题栏的 Agent 状态与“连接 Agent”按钮进入统一接入中心；日常用户也能从这里
  刷新、修复或移除接入，不再依赖隐藏入口。
- 只展示当前后端真实支持的 Claude 与 Codex，不展示 Kimi、Custom Agent 或
  “即将支持”等无功能占位。
- Provider 卡片展示真实检测来源、配置路径、状态和下一步。安装、修复、卸载、
  刷新均调用 `/api/v1/setup`；Runtime 离线时按钮禁用并显示真实原因。
- Codex 信任仍由用户在官方 Codex `/hooks` 界面完成；ActRealm 只复制 Runtime
  返回的命令并在用户回来后重新检测，不伪造自动信任。
- “查看接入指南”直接打开 GitHub 中维护的中文用户指南。

## Web 控制界面明确排除

- Web 控制界面按设计移除“台前调度”页面及其入口；macOS 原生客户端仍将其作为
  依赖辅助功能权限的实验性能力，不改变 Runtime 的任务或审批状态。
- 移除 HUD 胶囊和系统通知；不申请 macOS 通知权限。
- 不直接展示原始 Hook JSON，不把秘密答案写入 SQLite、日志或导出。

## macOS 原生客户端与 Web 的功能对齐

原生 macOS 客户端已改为直接消费 Web 使用的同一组认证 Runtime
合约，不另存一份设置或会话状态。除“台前调度”依然是 macOS
专属能力外，当前原生界面已接回：

1. 首次进入状态、标题栏 Agent 状态和 Claude/Codex 统一接入中心；
2. 安全安装、修复、移除、Codex `/hooks` 信任步骤和中文指南；
3. Claude AskUserQuestion/Elicitation 与已托管 Codex requestUserInput 的真实表单；
4. Provider 原生审批的只观察卡片，以及原界面交回，不伪造批准/拒绝；
5. 任务的官方标题、活动/本轮时间、Token、上下文、估算价格、环境、计划、工具、
   子 Agent、恢复/控制状态、跳转和 Connector 接管；
6. 按 Runtime 返回的任意额度窗口动态显示，包括来源、套餐和真实重置时间；
7. 通知规则、提示音、保留期、JSON/统计导出、`DELETE` 清除和 Runtime 恢复；
8. Claude 额度桥、Codex 增强 Hook、安全字段白名单与 Provider 静音。

原生端的密码和未提交问题答案只保存在 SwiftUI View 的内存状态中；
任何 Provider 能力仍由 Runtime snapshot/capability 决定，不从 macOS 界面推测。

## 必须保留的安全例外

即使设计示例没有完整覆盖，生产界面仍需显示真实状态：

- Provider 原生审批只能回原界面处理；ActRealm 只有持有活跃 waiter 时才能显示批准/拒绝。
- Claude AskUserQuestion、Elicitation 和 Codex requestUserInput 按真实 Schema 渲染单选、多选、自由输入和秘密字段。
- Runtime 离线、重连、额度不可用/过期必须明确显示，不能沿用最后一次在线文案。
- “重启”只安全重启本地 Bridge 通道：旧 waiter 先 fail-open，再重绑 0600 Socket；不伪装成操作系统级进程监督器。

## macOS 原生界面待验收

1. 在包含匹配 SwiftUI 宏插件的 Xcode 工具链上完成全量编译和快照。
2. 检查最小窗口、三栏工作区、接入中心、两个设置页和交互问题长内容。
3. 使用新的 Claude/Codex 真实会话验收安装、信任、问答、额度、跳转与 Connector。

## 本地验收门禁

- JavaScript 语法、Rust 格式和 diff whitespace。
- 全工作区 Clippy `-D warnings`。
- 全工作区测试和 release 构建。
- Claude/Codex 五轮控制链路、Bridge 重启期间 fail-open、重启后再次审批。
- 1600×600 浏览器视觉检查、任务展开、通知与数据页和浏览器错误日志。
- 精确两分钟 CPU/RSS 稳定性门禁。

本次单标题栏精简属于纯前端增量：JavaScript 语法、Rust 格式、diff
whitespace、嵌入式 UI 回归测试和 workspace release 构建均通过，并完成
本机视觉验收。完整 M14 门禁证据见 `M14_USAGE_CONTEXT_QUOTA.md`。
