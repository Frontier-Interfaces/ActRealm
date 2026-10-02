# ActRealm H3 Attention、活跃/历史分离验收

日期：2026-08-21

分支：`agent/h4-h5-control-plane`

状态：实现与自动化门禁通过；精确提交 `9dfd0e1208cc2d84c0f5cd436c04d2632a947e4b`
的 build 78 已完成 Apple Development 签名、安装和实机返工验收。

## 结论

H3 的最小完整纵向切片已经接通：活跃看板只保留仍在运行、等待处理、失败或处于完成保留期
的任务；离开活跃看板的真实任务进入独立历史中心。历史列表只读取有界安全摘要，选中任务
后才按需读取 Review 和 Checkpoint；Diff、Prompt、命令、工具输入输出、Transcript 和本机
路径不进入历史列表响应。

归档、删除历史和 Provider 控制已经拆成不同语义：

- 归档只允许已结束且没有可回复 Attention 的任务，不会停止 Provider；
- 运行中或仍等待审批/回答的任务，Runtime 返回 `TASK_STILL_ACTIVE`；
- 归档后的延迟工具/状态事件不会让旧任务回流，只有新 Prompt 或已验证 Connector 的新活动
  Turn 才会回到活跃看板；
- 删除历史会删除 ActRealm 本机的任务事件、Review 基线、Checkpoint 元数据和会话级用量明细，
  但不修改工作区文件、Commit、分支或 Provider 会话；历史日级用量汇总仍保留；
- UI 的删除按钮在最终动作前显示不可误解的破坏性确认。

## 本次实现

### Runtime

- 新增本机认证接口 `GET /api/v1/history`，最多返回 500 个历史摘要；
- 新增 `POST /api/v1/sessions/{id}/archive`；
- 新增 `DELETE /api/v1/sessions/{id}/history`；
- 历史摘要仅包含任务身份、Provider、项目、模型、最终状态、时间、分支、最新验证状态、
  Checkpoint 数量、安全事件数量和跳转能力；
- 查询排除正在活跃的任务和只有 Provider 生命周期、没有有效任务活动的记录；
- 新增共享 API 错误合同和中英文客户端文案。

### macOS

- 主窗口保留原工作区，在顶部增加独立“历史”入口；
- 历史中心支持任务/项目/模型/分支全文检索，以及项目、模型、Git 分支、Provider、状态、
  验证和日期的组合筛选；
- 详情显示最终状态、Review、验证证据、Checkpoint、安全事件摘要和真实跳转能力；
- 活跃任务菜单把旧“清除”改为“归档”，运行中任务的归档按钮禁用；
- 删除确认明确说明不会停止 Provider，也不会删除工作区、Commit 或分支。

### Web parity

- Web 活跃任务操作同步使用 Runtime 归档接口；
- 运行中或等待回复的任务不可归档；
- 删除了依赖浏览器本地隐藏版本来冒充持久化归档的主路径。

## 自动化证据

完整门禁在本次实现上通过：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline
cargo build --workspace --release --offline
./scripts/check-actrealm-language.sh
TZ=UTC apps/macos/Scripts/test.sh
```

新增回归覆盖：

- 历史查询不重复活跃任务、不返回纯生命周期噪声并遵守 limit；
- 运行中任务归档失败，完成提醒在归档时按正确语义解决；
- 删除历史清除任务明细并保留日级 Token 汇总；
- HTTP 真实路径覆盖运行拒绝、结束归档、历史查询和历史删除，返回值明确证明
  `gitChanged=false`、`providerStopped=false`；
- Swift 解码测试保证历史合同不携带 Prompt、路径或工具内容。

最终全量结果包含 Rust Runtime 55 个单元测试、Runtime 集成 77 个测试、Server 56 个单元
测试、macOS 192 个 Swift/XCTest 测试及其他工作区测试，全部通过；显式两分钟资源 soak 仍按
既有计划保持 ignored，不在 H3 重复执行。

## 实机 Computer Use 验收

安装 build 77 发现并返工生命周期噪声后，使用精确提交重新打包并安装 build 78；最终 Doctor
`overall=pass`。实机完成以下只读或可恢复操作：

1. 从原工作区进入历史中心，返回工作区正常；
2. 初次发现 261 条记录中混入纯生命周期“未命名任务”，返工后收敛为 114 个真实任务；
3. Provider 切换到 Claude Code 后得到 42 项；
4. 搜索 `Slugify` 后精确得到 1 项，分支、模型、Review 和验证状态一致；
5. 打开删除确认，确认文案准确，随后取消，没有删除真实历史；
6. 打开当前运行任务操作菜单，“归档”按钮为 disabled，运行任务仍在活跃看板；
7. Runtime 状态本机在线，现有 Outbox、任务、额度和 Token 面板未被历史中心替换。

## 数据和性能边界

- 历史列表一次最多 500 项，主快照不读取全部历史；
- Review 和 Checkpoint 只在选中后加载，Diff 继续使用原有按需本机接口；
- 历史 API 不进入 Companion/Cloud/Display 投影；
- 删除历史不会回写或清理 Git；
- 旧 build 75/76/77 已移动到废纸篓，未执行不可恢复删除。

## 后续

H3 之后继续按照总计划推进 H6 诊断/来源可信度收口，再进入 H7 Display 协议投影。手机、
Apple Watch 和 Claude Cowork 仍只保留在延期计划中，不在本次实现。
