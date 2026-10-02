# ActRealm H5 Checkpoint 与恢复检查点

日期：2026-08-18

分支：`agent/h4-h5-control-plane`

候选：ActRealm `0.1.0 (69)`，Runtime SQLite schema 34，Apple Development 签名

结论：H5 通过。真实 build 68 UI 已创建“H5 实机验收”metadata Checkpoint，显示分支
`agent/h4-h5-control-plane`、HEAD `fce0a0f024e5`、1个工作区改动和历史 validation；当前任务
没有可验证 jump locator，因此“恢复会话”正确禁用。默认 Checkpoint 只写入 ActRealm 本机
元数据；只有用户明确点击并再次确认时才创建 stash-like Git 对象。恢复前必须通过 dry-run，
任何未提交改动、仓库身份变化、分支/HEAD 漂移、对象变化或补丁冲突都会阻止执行。

## 1. Checkpoint 数据合同

schema 34 新增本地私有 `task_checkpoints`：

- session/turn、Provider 和恢复能力；
- branch、完整 HEAD、worktree kind、dirty 和文件计数；
- Review baseline 时间；
- 当时最近10项结构化 test/build validation；
- 可选 Git snapshot object、ActRealm 自有 ref 和 patch SHA-256；
- 可选安全标签和创建时间。

仓库路径、repository identity、Provider session identity、Git ref 和完整对象ID不进入普通 UI、
Companion、Cloud 或 Display。用户显式导出本机 JSON 时 repository path/identity 仍会脱敏。

## 2. 四类 Checkpoint

1. Metadata：默认入口，只保存本机元数据，不 commit、不 stash、不改工作区；
2. Git：显式创建 `git stash create` 对象并固定在 `refs/actrealm/checkpoints/<uuid>`，不切分支、
   不更新 index、不改工作树；只包含 tracked changes；
3. Provider：冻结当时的 Provider resume capability；恢复时重新确认原 session 仍存在；
4. Validation：冻结当时结构化状态，恢复后始终标为 historical，新 Turn 必须重跑。

没有实现自动 commit，也没有默认自动 Worktree；这两项只有未来独立设置与用户明确授权后才能
增加。未跟踪文件不会偷偷读取或打包，UI明确显示`untracked_not_captured`。

## 3. 恢复动作严格分离

- 恢复会话：只跳回 exact conversation、terminal 或 application，不修改 Git；
- 恢复代码：要求仓库身份、branch、HEAD一致且当前工作区干净，然后先`git apply --check`，
  再应用 checkpoint patch；
- 回退代码：要求当前 tracked diff 的SHA-256与checkpoint完全一致、没有untracked文件，然后
  先做reverse check再反向应用；
- 删除 Checkpoint：只删除ActRealm DB元数据和ActRealm自有Git ref，不删除Provider会话、
  branch、commit或工作区文件。

所有 Git 子进程继续使用绝对`/usr/bin/git`、禁用终端提示、750ms边界和1MiB输入/输出上限。
恢复没有`reset --hard`、`checkout --`或其它 destructive reset。

## 4. Dry-run 阻断条件

- repository/worktree不存在；
- repository identity变化；
- branch或HEAD变化；
- 恢复代码时存在任何未提交改动；
- 回退代码时存在untracked文件；
- 当前diff不等于checkpoint patch；
- Git对象/patch digest变化；
- `git apply --check`或reverse check冲突；
- Provider原会话已不可用。

阻断原因使用稳定枚举，macOS显示可选择文本；失败保持工作区原样。

## 5. UI 与 Web

- 活动任务Review新增Checkpoint入口；
- Sheet支持标签、元数据Checkpoint、显式Git快照、列表、证据、preflight、恢复会话、恢复代码、
  回退代码和删除；
- Git快照、删除和每项恢复动作都有独立确认；
- Web Review使用相同本机API，所有动态值只通过`textContent`渲染；
- H3独立历史中心后续复用相同列表/详情API，不复制第二份恢复逻辑。

## 6. 自动化矩阵

- metadata checkpoint创建、重启持久、读取、删除且不删除session；
- 本机JSON导出脱敏checkpoint仓库path/identity；
- Git snapshot不改变工作区，untracked明确不捕获；
- dirty worktree阻止restore；
- 用户在checkpoint之后增加新修改时阻止rollback且文件字节不变；
- 完全匹配时restore与rollback各自成功；
- branch漂移阻止恢复；
- 删除后ActRealm自有Git ref消失；
- 非Git项目允许metadata，拒绝Git snapshot；
- macOS旧合同兼容和新Checkpoint/preflight解码；
- Companion/Cloud snapshot不包含Checkpoint。

## 7. schema 34 备份

升级前 SQLite online backup：

`~/.actrealm/token-backups/h5-checkpoint-schema34-2026-08-18/data-before-schema34.sqlite`

- mode `0600`，目录 mode `0700`；
- user version 33；integrity `ok`；
- SHA-256：`5f9237a72727b261541119ae193c22318f4b787967061068d0419556c5dfc61b`。

## 8. 验收边界

H5不承诺Provider一定能恢复原对话，也不承诺Git对象永久免于用户自己的GC/仓库删除。它承诺
在能力存在时准确执行，在能力或安全条件不足时明确拒绝，并且不会为了“恢复成功”覆盖用户
后续修改。H3历史中心尚未开发；build68已从活动任务Review验证metadata纵向切片，Git对象
创建、restore和rollback由隔离临时仓库自动化验证，没有在用户当前工作区执行恢复动作。

提交前门禁：Runtime单元48项、Runtime集成76项、Server单元56项、macOS 190项/27 suites、
Web/shared 12项、Clippy `-D warnings`、全workspace、release、语言合同、128KiB Web预算与
Info.plist全部通过。
