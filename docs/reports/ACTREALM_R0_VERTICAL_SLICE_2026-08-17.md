# ActRealm × Display R0 真实闭环验证

日期：2026-08-17

结论：**通过，但 Claude Code 成功任务链路仍受本机 OAuth 过期阻塞。** Codex、
Companion v2、真实 Attention、Runtime 重启恢复、长条副屏布局和历史任务过滤均已在
本机真实安装包中验证；Claude Code 的真实鉴权失败路径和错误处理已验证，成功路径须在
用户重新登录 Claude Code 后补测。

## 1. 验证环境

- macOS 本机安装：`/Applications/ActRealm.app`、Display 应用
- 真实副屏：`TYPEC`，2880 × 864，60 Hz
- ActRealm：0.1.0 (29)，Apple Development 签名，arm64
- Display：0.31.0 (52)，Apple Development 签名
- Companion：loopback only，发现文件权限 `0600`，schema 1，Runtime public protocol 2
- Display 授权 scope：`snapshot.read`、`session.jump`、`attention.respond`

旧安装包均移动到仓库忽略的 `outputs/r0-backup/`，没有覆盖用户数据，可以人工回滚。

## 2. 已验证结果

| 项目 | 结果 | 证据 |
| --- | --- | --- |
| Runtime v2 | PASS | 健康接口返回 protocol 2；应用替换后发现文件生成新 instanceId |
| Companion 授权保持 | PASS | 两次 Runtime 重启及两次应用替换后 registration ID 与 scopes 不变 |
| 自动重连 | PASS | Runtime 退出时 Display 显示恢复中并保留最后真实状态；重启后自动恢复在线 |
| Codex 真实任务 | PASS | 项目、模型、prompt、当前动作、3/5 结构化计划与增量活动均显示 |
| Codex 真实审批 | PASS | 真实 Bash approval 进入 OUTBOX；Allow + 二次确认 + 3 秒撤回后 Provider 继续并生成证明文件 |
| Claude Code | PARTIAL | CLI 2.1.226 hooks 正常；OAuth 已过期，真实任务产生 authentication_failed，Display 可确认并移除 |
| 任务生命周期 | PASS | 完成/失败/空闲且无当前 Attention 的任务立即离开主列表；长时间运行任务不按年龄隐藏 |
| 遗留活跃假象 | PASS | Runtime payload 中 7 月遗留 `tool_running` 且进程已消失的记录，由 recovery/liveness 派生层过滤 |
| OUTBOX 队列 | PASS | 无请求时整个 OUTBOX 区域不渲染；有多个请求时一次只显示最早一项并提示剩余数量 |
| 工作流可读性 | PASS | 当前 Turn 安全活动增量刷新；精确工具名可见；列表有独立滚动区域 |
| 自适应布局 | PASS | 780×900、1280×720、1920×1080、2880×864、3840×2160 分类测试通过；真实 2880×864 输出通过 |
| 安全边界 | PASS | Display 不读取 SQLite，不持有 Provider Cookie；活动接口不返回命令参数、工具输入输出或文件内容 |

## 3. 测试结果

- ActRealm macOS：174 tests / 26 suites，0 failures。
- ActRealm 任务投影定向测试：8 tests / 2 suites，0 failures。
- Display XCTest：175 tests，1 个按设计 skip，0 failures。
- Display Swift Testing：28 tests / 4 suites，0 failures。
- 两个 `.app` 均通过 `codesign --verify --deep --strict`。
- Display release ZIP 完成签名、解包和二次 bundle 校验。

## 4. 资源与日志基线

真实任务和副屏输出同时活跃时的一次瞬时采样：

| 进程 | RSS | CPU（瞬时） |
| --- | ---: | ---: |
| ActRealm macOS | 约 73 MB | 约 2% |
| ActRealm Runtime | 约 34 MB | 约 16% |
| Display | 约 187 MB | 约 20% |

这不是空闲基准或 SLA，只用于后续同机回归比较。当前 Codex 任务持续产生事件，且
Display 正在驱动 2880×864 输出，所以不能把该 CPU 数字解释为空闲占用。

Companion 快照日志已由“每秒记录”改为只在 Runtime instance、session 集合或可见任务
集合发生结构变化时记录。最终包启动并稳定同步后只产生一次结构日志，不再随 capturedAt
或状态轮询刷屏。

## 5. 本轮发现并修复的问题

1. ActRealm 主窗口原先会保留完成/空闲任务 30 分钟，与用户要求冲突；现在无当前请求的
   非活跃任务立即隐藏，数据保留策略不受影响。
2. Display 不能只依赖 `execState=tool_running` 判断活跃；旧进程死亡但数据库仍残留该状态
   时会产生幽灵任务。Runtime 现在会在 managed connector 尚未恢复 thread 时使用 Hook
   留下的 Provider PID 细化恢复态：真实存活进程为 `observing`，死亡或身份复用进程为
   `lost_control`；Display 同时拒绝未获新证据的 `waiting_for_event`。
3. OUTBOX 空态仍占据长条屏右侧整列；现在零请求时完全不参与布局。
4. 多个 Attention 曾同时展示并争夺视觉焦点；现在按 oldest-first 一次展示一个，处理后
   自动推进下一项。
5. Companion 协议文档的活动游标误写为 snake_case；已改为真实参数
   `afterIngestSequence`。

## 6. 已知限制与后续入口

- Claude Code 当前 `authMethod=none`，成功任务、授权和 plan parity 必须在用户重新登录后
  补测；应用不能代替用户完成 Provider 登录。
- Runtime snapshot 仍携带若干 `waiting_for_event` / `lost_control` 历史 session；UI 已正确
  过滤，但 R1 应把“存储历史”和“实时投影”在服务端进一步分层，减少 payload 与派生成本。
- 本轮只完成一次人工 Runtime 重启恢复；路线图原定的五轮睡眠/唤醒和多次重启属于 R6
  soak 测试，不能宣称完成。
- 当前资源数字是活跃瞬时采样；R4 仍需两小时空闲/活跃分段、主线程卡顿、数据库增长和
  timeline p95 的可重复测量。

后续优先级、验收标准与停止条件见
`docs/PRODUCT_IMPROVEMENT_ROADMAP_2026-08-17.md` 的 R1–R6。下一阶段先做 R1 的统一
状态真相与 current-turn 工作流语义，再做 R2 的完整 Attention 状态机；不继续增加装饰性
AI 动画或新的 Provider。
