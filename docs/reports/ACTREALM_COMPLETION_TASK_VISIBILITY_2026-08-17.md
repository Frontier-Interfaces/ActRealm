# ActRealm 已完成任务隐藏策略实现报告

日期：2026-08-17

## 结果

ActRealm 已完成两种互斥的已完成任务隐藏策略，默认行为不变：任务必须由 Runtime 通过
Provider 事实确认完成，用户点击“确认完成”后才隐藏。用户也可以选择完成后自动隐藏，
并在 5、15、30、60 分钟中选择保留时间，默认 30 分钟。

本轮没有修改 Display 仓库或已安装的 Display 应用。Runtime 对 Companion 快照增加的
`autoHideAt` 是向后兼容的可选字段，供 ActRealm 交互验收通过后再由 Display 使用。

## 事实与状态边界

- Runtime 是完成事实、自动隐藏截止时间和关闭原因的唯一来源；macOS 与 Web UI 不根据
  “多久没有事件”自行判断任务完成。
- 只有 `kind = completion` 且仍为 `open`/`snoozed` 的待办可以因策略到期关闭，关闭原因
  为 `auto_hidden`。
- running、审批、问题、错误和 Provider 原生等待不会因为 30 分钟没有新事件而隐藏。
- 倒计时内出现新 Turn、continuation 或其他明确活动时，旧 completion 会以
  `superseded_by_activity` 关闭；新的完成事实会建立新的截止时间。
- 隐藏不删除 session、事件、工作流、计划或 Token 统计；手动删除任务仍是另一项独立
  操作。
- 默认策略为 `afterConfirmation`。旧数据库升级后保持默认策略，避免静默改变现有行为。

## 实现范围

- SQLite schema 升至 27，为 `attention_items` 增加可空的 `auto_hide_at` 及查询索引；旧库
  通过幂等迁移补列，不丢失现有数据。
- Runtime 设置写入和现有 completion 截止时间调整位于同一事务；切换回手动确认会清除
  尚未关闭的自动隐藏截止时间。
- Runtime 在快照缓存需要刷新时处理 snooze 到期和 completion 自动隐藏，最长约 2 秒
  反映到客户端，不需要依赖 UI Timer 或 UI 进程持续运行。
- macOS 设置页新增“已完成任务”区域，完成卡片显示剩余分钟；Web 设置页提供相同策略。
- Companion projection 增加可选的 `autoHideAt`；旧客户端可以忽略该字段。
- 同时补齐分支中两个时间线 API 错误码的共享中英文契约，并把时间线读取选项封装为
  参数结构，使全工作区 Clippy 门禁保持零警告；时间线行为未改变。

## 回归证据

- `cargo fmt --all -- --check`：通过。
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过。
- `cargo test --workspace --offline`：通过；仅保留仓库原有的显式手动/资源门禁 ignore。
- `cargo build --workspace --release --offline`：通过。
- `TZ=UTC apps/macos/Scripts/test.sh`：177 tests / 26 suites，通过。
- `node --test web/i18n.test.js`：5 项通过。
- `node --test shared/contracts/schema-contracts.test.mjs`：17 项通过。
- `./scripts/check-actrealm-language.sh`：通过，57 条 Runtime message、82 条 API error。
- `plutil -lint apps/macos/Resources/Info.plist`：通过。
- 本机候选：`/Applications/ActRealm.app` 已更新为 build 37，Apple Development 签名验证
  通过，前端与 bundled Runtime 均正常运行。
- 真实 UI：设置页默认显示“确认后隐藏”；切换“自动隐藏”后正确出现默认 30 分钟选择
  器，再切回默认策略后 SQLite 确认为 `afterConfirmation | 30`，schema version 为 27。

## 未包含

- Display 的设置界面、任务卡和倒计时展示尚未修改。
- 本轮没有把隐藏解释成停止 Provider 任务，也没有新增删除历史数据的行为。
- 本轮没有执行 commit、push、tag 或公开发布；本机安装只作为 QA 候选。

## 体验反馈后的待修正项

build 37 的自动模式仍把“确认完成”和“立即隐藏”绑定在一起。用户实际需要的是：点击
“知道了/确定”后只清除 Outbox 提醒，任务继续显示到原 `autoHideAt`，到期后才隐藏。
后续 A0/A1 将把提醒已读状态与任务可见性 deadline 分离；本报告上方仍记录 build 37 的
实际实现，不把计划中的行为冒充为已经完成。

## 后续状态

上述待修正项已在 build 38 / SQLite schema 28 完成。新的实现、测试和本机安装证据见
`ACTREALM_A1_ATTENTION_VISIBILITY_2026-08-17.md`；本报告继续保留 build 37 的历史事实。
