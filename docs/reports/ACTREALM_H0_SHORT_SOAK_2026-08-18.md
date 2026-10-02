# ActRealm H0 十分钟短时资源与恢复检查

日期：2026-08-18

分支：`agent/runtime-v2-agent-observability`

安装候选：ActRealm `0.1.0 (59)`，应用提交
`f3e1e3689995432f0d4384f4206cd9fdd7a722e4`

结论：**H0 十分钟短时门通过；不构成两小时或长期稳定性证明。真实物理睡眠/唤醒和长期
soak 按用户决定延后到 H9。**

## 1. 现场负载

- `/Applications/ActRealm.app` 与内嵌 Runtime 真实运行，不是空白临时数据库；
- 起始时有两项真实 Codex 任务同时运行；
- 测试中一项任务真实转为 completion/等待确认，另一项继续运行；
- Outbox 从 0 变为 1，显示自动隐藏剩余时间；
- 测试没有点击“知道了”、打开应用、清除或处理任何用户 Attention；
- Token 页面起始为“历史数据部分可用”。

## 2. 采样方法

脚本：`scripts/h0-short-soak.sh`

- 请求时长：600 秒；
- 采样间隔：5 秒；
- 有效样本：118；
- 每个样本读取 macOS App/Runtime CPU、RSS、文件描述符、SQLite/WAL 大小，以及
  canonical/daily/model 三层 Token 总计；
- 开始和结束分别执行 SQLite `PRAGMA integrity_check`；
- Runtime RSS 短门预算：81,920 KiB；
- 进程退出、三层账本不一致、完整性失败或 Runtime 超预算会使脚本失败。

本地原始样本：
`/tmp/actrealm-h0-10m-build59-20260818-1434/samples.csv`。该路径是临时本机证据，不进入
仓库或云端。

## 3. 结果

| 指标 | 起始 | 峰值 | 结束 |
| --- | ---: | ---: | ---: |
| macOS App RSS | 99,472 KiB | 111,728 KiB | 104,496 KiB |
| Runtime RSS | 19,664 KiB | 23,232 KiB | 20,272 KiB |
| macOS App FD | 94 | 94 | 93 |
| Runtime FD | 22 | 24 | 22 |
| SQLite | 12,173,312 bytes | — | 12,197,888 bytes |

- SQLite 净增长：24,576 bytes，可由测试期间真实任务/Token 事件解释；
- WAL 结束大小：4,177,712 bytes；
- 三层 Token mismatch：0 / 118 样本；
- 完整性：开始 `ok`，结束 `ok`；
- App 平均 CPU：14.328%；Runtime 平均 CPU：8.637%。这是两项真实活跃任务、Token 增量
  解析和 Computer Use 观察下的活跃负载，不适用于空闲 CPU 门；
- RSS 和 FD 在区间内波动并回落，没有在十分钟样本中呈阶梯式持续增长。

## 4. 受控重启恢复

重启前：

- 两项任务：1 running、1 waiting；
- Outbox：1 个 completion；
- Token 状态：历史数据部分可用。

通过正常退出并重新启动 ActRealm 后：

- macOS App PID 从 `88700` 更新为 `1915`；
- Runtime PID 从 `88705` 更新为 `1918`；
- Runtime 恢复为本机在线；
- 两项任务、1 个 completion 和原自动隐藏语义均恢复；
- 没有恢复任何 approval/native approval waiter；数据库开放 Attention 只有
  `completion|open|1`；
- 三层账本均为 `4,299,105,541` Token；
- SQLite integrity 仍为 `ok`；
- 历史 collector 按设计进入“首次扫描中”，同时继续服务上一代已提交账本，没有归零或
  发布历史前缀。

## 5. 限制与后续

- 用户将 H0 运行时间从两小时改为十分钟；本报告只支持短时门；
- 没有主动让 Mac 进入真实系统睡眠，因为这会中断当前 Agent 任务；物理睡眠/唤醒、长时
  RSS、网络切换和 7 日行为留给 H9；
- 本轮只有真实 Codex 任务处于活动状态，Claude 额度与历史数据仍在 UI 中，但没有人为
  启动新的 Claude 任务制造覆盖；
- H0 短门通过后进入 H1 `FactEnvelope v1` 和任务事实可信度实现。
