# ActRealm H1 任务事实可信度检查点

日期：2026-08-18

分支：`agent/runtime-v2-agent-observability`

候选：ActRealm `0.1.0 (60)`，Apple Development 签名

结论：**H1 通过，进入 H2 ActRealm Review v1。**

## 1. 交付内容

新增 `SessionFacts v1`，为现有 session 值提供五组独立元数据：

- plan；
- activity；
- current target；
- completion；
- control。

每组事实包含：

- `sourceKind`：authoritative / observed / derived / unavailable；
- `sourceId`：受限、版本化的 Runtime/Provider 来源标识；
- `capturedAt`；
- `freshness`：live / delayed / stale / expired；
- `verification`：verified / partial / unverified / not_applicable；
- `absenceReason`；
- `capability`：direct / return_to_provider / observe_only / unavailable。

事实层是 additive metadata，不替换既有 plan、activity、target、completion 或 control 值，
旧客户端可以安全忽略。

## 2. 单一来源与缺失规则

- Provider capability 继续由现有 `provider-capabilities.json` 决定，没有建立第二套能力表；
- plan 只有当前 Turn 的真实步骤才标 authoritative；Turn 结束后标 `no_current_turn`；
- activity 的工具生命周期是 observed，Runtime reducer 是 derived；来源过期后不继续显示为
  实时事实；
- current target 只接受 Provider 明确字段产生的 allowlisted basename；
- completion 只在 Runtime terminal reducer 看到真实结束/失败状态时标 verified；
- control 只有 live waiter、有效期限和 `remoteActionable` 同时成立时才是 direct；
- native/provider-owned Attention 只能 return_to_provider；
- attached Connector 本身只证明 observe_only，不能冒充逐请求控制能力。

缺失原因冻结为：Provider 未提供、不支持、能力未知、无当前 Turn、无当前活动、无当前工具、
当前工具无目标、任务未完成和来源过期。

## 3. 合同与隐私

- 新增 `shared/contracts/fact-envelope.schema.json`；
- schema 拒绝未知字段、未知能力、伪 confidence 百分比和不符合受限 source ID 的值；
- Companion v2 snapshot 明确投影 `facts`；
- fact metadata 不包含 Prompt、路径、命令、tool input/output 或 Provider 原始 payload；
- server 回归使用私密 Prompt/路径 fixture，确认序列化事实中不存在这些内容。

## 4. macOS 与 Web

任务展开后按用户已经启用的模块显示：

- 计划事实；
- 活动事实；
- 目标事实；
- 完成事实；
- 控制事实。

界面显示受限来源类别、新鲜度、验证、缺失原因、控制边界和更新时间，不显示原始 source
payload。旧 Runtime 没有 `facts` 时保持原界面，不因 decode 失败丢掉整个 session。

## 5. 真实 build 60 验证

运行任务 `actrealm 优化` 展开后显示：

- 活动事实：`Provider 事件 · 实时 · 已验证 · 刚刚`；
- 控制事实：`Provider Connector · 实时 · 部分验证 · 刚刚`。

完成任务 `介绍 ESP3-S3-N16R8` 展开后显示：

- 活动事实：`无可用来源 · 已失效 · 不适用 · 当前 Turn 已结束`；
- 完成事实：`Runtime · 延迟 · 已验证`；
- 控制事实：`无可用来源 · 已失效 · 不适用 · 当前 Turn 已结束`。

这验证了完成后的旧 activity/control 不再冒充当前实时事实，同时完成依据仍被保留。主卡片
没有增加新行，信息只在用户展开后显示。Display 仓库未修改。

## 6. 自动化门

- Rust server：48 项通过；
- macOS：186 项 / 27 suites 通过；
- Web + shared schema：25 项通过；
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过；
- `cargo test --workspace --offline`：通过；
- `cargo build --workspace --release --offline`：通过；
- ActRealm 与 Runtime language contract：通过；
- `git diff --check`：通过；
- build 60 arm64 Apple Development 深度签名：通过。

第一次 workspace 运行中 `prompt_decision_can_be_undone_without_reaching_provider` 出现一次临时
socket 未就绪；该测试单独复跑通过，随后完整 workspace 再次运行全部通过。没有把第一次
失败隐藏或直接标绿。

## 7. 边界与下一步

H1 只证明任务事实的来源和生命周期，不等于 Review v1。它还没有提供 Git diff、测试运行、
Commit/PR、工作区归因或最终证据卡。下一阶段 H2 在本事实合同上实现 Review v1，不从自然
语言猜测“测试通过”。
