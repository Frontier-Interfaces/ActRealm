# ActRealm build 70 实机验收返工

日期：2026-08-18

状态：**build 70 自动化门与 Computer Use 实机回归通过。**

## 返工来源

build 69 的真实 UI 验收发现四项问题：新 Turn 仍可短暂保留上一 Turn 的 Review、桌面环境
上下文被错误截成任务摘要、多 Git 仓库工作区在全部 clean 时无法继续关联已经验证过的仓库，以及
Token 仪表板会被 AppModel 的一秒显示时钟反复重绘。Claude OAuth 同时被重新核对，以确认
重置时间缺失究竟是解析问题还是上游空值。

## 修改

1. Review 的刷新身份变化后立即清除旧快照，并强制重新读取当前 Turn；运行中任务不再继续
   显示上一 Turn 的“已完成”和旧动作。
2. Runtime 在生成有界任务摘要前移除 `in-app-browser-context` 传输信封，只使用
   `## My request:` 后的用户请求；启动时清除已经持久化的错误内部上下文标题。
3. 同一 Provider 会话的新 Turn 在当前 cwd 只是多仓库父目录时，复用上一 Turn 已写入本机
   Review baseline 的仓库根目录；baseline 成功写入后也把该根目录保留为会话的本机 Review
   工作目录。没有历史证据时仍保持歧义降级，不按标题猜仓库。
4. Token 仪表板改为 Equatable 的有界渲染输入：只有 Token、Token 决策、相关设置或本机
   日期发生变化才重绘；AppModel 每秒时钟不再重建完整热力图和仪表板内容。
5. Claude OAuth 本机缓存的 `5h`、`7d` 与 `extra_usage` 均真实返回
   `resetsAt: null`。现有解析器对非空 RFC3339/epoch reset 已有测试，因此继续显示
   “Provider 未提供”，不生成估算重置时间。

## 自动化证据

- Rust 新增：环境上下文标题过滤、旧标题清理、上一 Turn 嵌套仓库复用和 pending baseline
  路径测试；
- Swift 新增：同一天内的一秒时钟变化不会改变 Token 仪表板渲染输入；
- `cargo fmt --all -- --check`：通过；
- `cargo clippy --workspace --all-targets --offline -- -D warnings`：通过；
- `cargo test --workspace --offline`：停止旧安装 Runtime 后通过；
- `cargo build --workspace --release --offline`：通过；
- `./scripts/check-actrealm-language.sh`：通过；
- `TZ=UTC apps/macos/Scripts/test.sh`：191 项 Swift 测试通过；
- `plutil -lint apps/macos/Resources/Info.plist`：通过。

## 实机回归结果

Apple Development 签名的 build 70 在 `/Applications/ActRealm.app` 真实运行，Runtime 在线，
SQLite schema 34 且 integrity 为 `ok`。Computer Use 验证结果：

- 已有错误环境上下文标题在启动修复后归零；当前任务不再显示内部信封。由于原始 Prompt
  按隐私合同没有保留，本 Turn 无法离线重建已经丢失的用户摘要；下一个真实 Prompt 将使用
  新过滤路径，自动化测试已覆盖精确文本。
- 展开任务后 Review 直接显示“运行中”，无需手动刷新；同时正确关联
  `work/ActRealm-Cloud`，显示 `agent/h4-h5-control-plane`、commit、变更与基线限制。
- 日视图、总计、Tokens/费用热力图和每日可访问文本均正常；连续使用期间元素身份稳定，未再
  出现 build 69 每秒刷新导致的控件失效。
- 打开仪表板前 App RSS 末值 117.2 MB；关闭仪表板后的 20 个样本从 158.0 MB 回落到
  145.5 MB，折叠任务后稳定在 144.2–145.6 MB，低于 build 69 同类测试末值
  198.1 MB。采样期间 Runtime 正在首次历史重建且任务持续活动，因此 CPU 只记录为活动
  负载，不作为空闲门或长期无泄漏结论。
- `H5 实机验收` metadata Checkpoint 跨 build 保留；Provider 不支持时恢复按钮仍禁用，
  数据库仍只有 1 个 metadata Checkpoint、0 个 Git ref。
- 本机数据库中 190 个 session 的内部环境标题计数为 0；最近十分钟没有 ActRealm/Runtime
  error 或 fault 日志。

## 结论边界

本次返工解决 build 69 已复现的四个问题。Claude reset 仍受 Provider `null` 限制；Token
首次历史重建结束后的空闲 CPU 和更长内存曲线继续归入 H9 soak，不把本次短样本扩大成长期
性能承诺。
