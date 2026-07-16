# Flow Agent v1 中文使用教程

本教程面向准备从 GitHub 源码安装和测试 Flow Agent v1 的用户。当前 v1
优先验证 macOS arm64/x64，Claude Code 与 Codex CLI 是正式支持的两个
Provider。界面运行在本机浏览器中，Runtime、数据库和 Hook 通信都留在本机。

> 当前 `agent/v1-full` 分支是供本地测试的发布候选版。30 分钟稳定性门禁
> 通过后可以用于用户测试，但在连续 48 小时门禁完成前，不应称为最终 v1
> Release。

## 1. 使用前准备

需要：

- macOS；
- Git；
- Rust stable 1.85 或更高版本（`rustc --version`）；
- Claude Code、Codex CLI 至少安装一个，并能从当前终端直接运行
  `claude` 或 `codex`。

Flow Agent 不会替你安装、启动或拥有 Claude/Codex 会话。它只接收 Provider
官方 Hook 事件，并在 Provider 发出权限请求时提供允许、拒绝或交还终端三种
操作。

### 1.1 终端与客户端支持矩阵

是否有数据不取决于界面长得像终端还是客户端，而取决于该会话是否在本机执行
已经安装并信任的 Hook：

| Provider 运行形态 | v1 状态 | 说明 |
| --- | --- | --- |
| Claude Code CLI（终端） | 已验证 | 当前 P0 合约与真实版本测试形态 |
| Claude Code Desktop 的 Local 会话 | 共享配置，预期兼容 | 官方说明 Desktop 与 CLI 共享 hooks/settings；仍建议用本教程的本地验收清单确认 |
| Claude Code 远程/云端会话 | 不支持本机控制 | Hook 不在本机时无法连接本机 Unix Socket |
| Codex CLI（终端） | 已验证 | 当前 P0 合约、信任流程与真实版本测试形态 |
| Codex 桌面客户端的本地任务 | 实验兼容，未认证 | 官方说明 App/CLI 共享配置层，但 v1 尚未完成客户端专属 fixture 与批准回写验收；不要把“可能运行”当成“已保证” |
| Codex 云端/Web 任务 | 不支持本机控制 | 云端任务无法连接本机 Runtime |

因此“Claude Code 终端 + Codex 客户端”时，Claude 终端会产生数据；Codex 只有
在客户端运行本地任务、实际加载同一份用户级 Hook 且 Hook 已信任时才可能产生
数据，当前发布候选版不对这一形态作正式承诺。需要稳定控制 Codex 时，请使用
已验证的 Codex CLI。

参考官方说明：[Claude Code Desktop](https://code.claude.com/docs/en/desktop-quickstart)、
[Claude Code Hooks](https://code.claude.com/docs/en/hooks)、
[Codex 配置层](https://learn.chatgpt.com/docs/config-file/config-basic) 与
[Codex Hooks](https://learn.chatgpt.com/docs/hooks)。

## 2. 下载并构建

```bash
git clone https://github.com/dlxdjj/flow-agent.git
cd flow-agent
git checkout agent/v1-full
cargo build --release
./target/release/flow-agent --version
```

构建结果是一个单文件程序：

```text
target/release/flow-agent
```

Web 界面已嵌入该二进制，不需要安装 Node.js，也不需要单独启动前端服务。

### 2.1 安装完成判定（硬性要求）

Flow Agent 由三个缺一不可的部分组成：

1. `~/.flow-agent/bin/flow-agent` 稳定程序；
2. 持续运行的本机 Runtime；
3. Claude/Codex 配置中已经安装并信任的 Hook。

**只拉取 GitHub、只构建程序或只修改 Provider Hook，都不算安装完成。** 安装者
必须逐项确认：

- `~/.flow-agent/bin/flow-agent` 已生成且可以执行；
- `flow-agent serve --open` 正在一个保持开启的终端中运行；
- 控制页是由当前这次 `serve --open` 自动打开的页面，不是旧书签、旧端口或直接
  打开的 `web/index.html`；
- `flow-agent doctor` 中 `runtime.control_loop` 通过；
- Codex 已由用户亲自在 `/hooks` 中信任 Flow Agent；
- 安装并信任后启动的全新 Codex 会话，能在面板中产生真实事件。

任意一项未通过时，不得向用户报告“已经安装完成”。其中“页面能显示”不代表
Runtime 在线：页面出现“Runtime 未连接”，通常表示提供页面和 WebSocket 的
`flow-agent serve` 进程已经退出，或者浏览器仍停留在上一次启动的随机端口。

### 2.2 可直接交给另一个 Agent 的安装指令

复制下面整段给负责安装的 Agent：

> 你正在本机安装 Flow Agent。以下是硬性要求，不得只修改 Codex 配置后就报告
> 完成：首先进入仓库并切换到 `agent/v1-full`，执行
> `cargo build --workspace --release --offline`；然后执行
> `./target/release/flow-agent install-hooks codex --enhanced-codex-activity`，确认
> `~/.flow-agent/bin/flow-agent` 已生成。接着必须在一个独立且保持开启的终端中
> 运行 `~/.flow-agent/bin/flow-agent serve --open`，不得关闭该终端，不得直接打开
> `web/index.html`，也不得复用旧的 localhost 地址。随后要求用户打开一个全新
> Codex 会话，输入 `/hooks`，逐项检查并亲自信任 Flow Agent；Agent 不得绕过或
> 代替用户完成信任。信任后再次启动全新 Codex 会话，并在另一个终端执行
> `~/.flow-agent/bin/flow-agent doctor`。只有 `runtime.control_loop` 通过、Codex
> Hook 配置/信任检查通过、且面板收到该新会话的真实事件，才允许报告安装成功。
> 如果网页显示“Runtime 未连接”，先判断 `serve --open` 终端是否仍在运行；若已
> 退出，关闭旧网页并重新运行 `~/.flow-agent/bin/flow-agent serve --open`，使用
> 它新打开的页面继续，不能把问题归因于网页文件或仅重复修改 Hook。

如果目标机器还需要 Claude，先确认 `claude --version` 可用，再额外执行：

```bash
~/.flow-agent/bin/flow-agent install-hooks claude
```

不要在未安装 Claude CLI 的机器上机械执行 `install-hooks all`；应只安装实际存在
的 Provider。

## 3. 首次启动

在一个单独的终端窗口运行：

```bash
./target/release/flow-agent serve --open
```

这会：

1. 启动本地 Runtime；
2. 只在 `127.0.0.1` 的随机端口启动控制界面；
3. 使用一次性地址在默认浏览器中完成本机身份交换；
4. 默认进入 `widget` 批准模式。

这个终端窗口需要保持运行。要停止 Runtime，回到该终端按 `Control-C`。
同一份数据目录只允许一个 Runtime 实例；如果提示已有实例，请使用已打开的
界面，或先停止原实例再重新执行 `serve --open`。

这里的网页地址每次启动都可能变化。旧页面出现“Runtime 未连接”时，应关闭旧
页面并重新执行 `serve --open`，使用命令自动打开的新页面；不要收藏随机端口作
为长期入口。当前 v1 尚未安装开机自启，电脑重启后也必须重新运行该命令。

## 4. 接入 Claude Code

### 界面方式（推荐）

1. 在首次运行窗口找到 Claude 卡片；
2. 点击“安全接入”；
3. Flow Agent 会先备份，再语义合并 `~/.claude/settings.json`；
4. 重新启动一次真实 Claude Code 会话；
5. 回到 Flow Agent 点击“刷新状态”；
6. 只有收到安装后的真实事件，状态才会变成“已接入”。

### 命令行方式

```bash
./target/release/flow-agent install-hooks claude
./target/release/flow-agent doctor
```

安装器只添加 Flow Agent 自己的 Hook，不删除用户原有 Hook，也不整份重写
未知配置。Provider CLI 不在 `PATH` 时，安装器会拒绝创建配置。

## 5. 接入 Codex CLI

Codex 比 Claude 多一个必须由用户亲自完成的信任步骤。

### 界面方式（推荐）

1. 在首次运行窗口点击 Codex 的“安全接入”；
2. 打开一个新的 Codex 会话；
3. 在 Codex 中输入 `/hooks`；
4. 逐项检查 Flow Agent 命令并选择信任；
5. 重新启动一个 Codex 会话；
6. 回到 Flow Agent 点击“刷新状态”，等待真实事件验证。

Flow Agent 不会修改 Codex 的信任状态，也不会绕过 `/hooks` 审查。如果 Hook
定义在升级后发生变化，Codex 可能要求重新信任。

### 命令行方式

```bash
./target/release/flow-agent install-hooks codex
./target/release/flow-agent doctor
```

首次运行界面默认安装增强工具活动 Hook，因此 Agent 任务能显示工具开始/完成。
命令行安装为了保持兼容仍默认使用较低噪声的轮级事件；若从命令行接入并希望
获得同样的实时活动，请显式开启：

```bash
./target/release/flow-agent install-hooks codex --enhanced-codex-activity
```

修改后需要再次在 Codex `/hooks` 中检查和信任。

同时接入两个 Provider：

```bash
./target/release/flow-agent install-hooks all
```

## 6. 日常使用

1. 先运行 `flow-agent serve --open` 并保持 Runtime 终端开启；
2. 像平常一样在其他终端启动 Claude Code 或 Codex CLI；
3. Agent 的当前任务标题、实时状态、项目名和需要关注的事件会出现在 Flow Agent；
4. Provider 发出权限请求时，待处理区域会出现操作卡片。

“Agent 任务”只展示以下会话：仍在运行、仍有待处理事项，或最后一次活动距今
不超过 30 分钟。结束且超过 30 分钟的会话仍可按数据保留设置存在本机历史中，
但不会继续占据主列表。标题来自最近一次用户任务的限长摘要，不再用用户名或
项目名冒充任务标题；Claude/Codex 行使用对应图像标识，不再显示 `Cl` / `Co`
字母占位。Provider 与项目名显示在次要信息行。活动行会按真实事件
显示思考计时、正在运行的工具、等待批准、完成、失败或空闲，缺少工具级 Hook
时只诚实显示轮级状态。

待处理卡片上的“在 Agent 任务中查看”会选择对应会话，将它置顶、高亮并滚动到
可见位置；即使该会话已经超过 30 分钟，只要仍有待处理事项也不会被过滤掉。

权限卡支持：

- **允许**：向 Provider 写回本次请求的允许结果；
- **拒绝**：向 Provider 写回本次请求的拒绝结果；
- **撤回**：允许/拒绝点击后有 3 秒提交等待，在写回前可以撤回；
- **去终端处理**：Flow Agent 不做决定，立即把本次请求交还 Provider 原生流程。

允许和拒绝只控制当前 `PermissionRequest`，不是永久授权。Flow Agent 不实现
“始终允许”，也不会绕过 Provider 策略、企业规则或沙箱。

界面中的常见状态：

- `等待决定`：还没有选择结果；
- `3 秒内可撤回`：决定尚未写回；
- `已发送`：指令已经交给 Provider，但不能伪称 Provider 已执行；
- `已确认`：后续真实 Provider 事件证明任务继续；
- `已交还终端`：需要回原终端继续；
- `已过期`：原等待者已经失效，不能再提交旧决定。

## 7. Runtime 离线和超时会怎样

Flow Agent 的故障原则是 fail-open：

- Runtime 不存在；
- Socket 无法连接；
- Runtime 中途退出；
- 返回数据损坏或请求 ID 不匹配；
- 用户主动选择“去终端处理”；
- Provider 专属等待期限到期；

这些情况都不会把 Agent 永久卡住。Hook 会保持 stdout/stderr 安静并把控制权
交回 Provider 原生终端流程。正常等待上限与 Provider 对齐：Claude 最长 24
小时，Codex 最长 1 小时；连接断开会立即交还。

## 8. 设置、通知与额度

右上角设置中可以管理：

- 浏览器通知、声音和免打扰；
- 本地事件保留 30、90 或 365 天；
- Codex 增强工具活动 Hook；
- Claude 可选的 status-line 额度桥；
- 本机使用统计、数据导出和彻底清除。

额度模块固定分成三项：`Claude · 5 小时`、`Claude · 7 天`、
`Codex · 本周`。缺少其中任一窗口时，只将该窗口标为不可用，不会拿另一个窗口
补位或虚构百分比。

Claude 没有自定义 `statusLine` 时可直接开启额度桥；已有自定义
`statusLine` 时，设置页会提供明确的“保留现有并开启”。只有点击该动作后，Flow
Agent 才会备份完整原对象、安装代理并继续显示原 status-line 输出；卸载额度桥
会把原对象逐字段恢复。不会静默覆盖，也不会把已有脚本的输出吞掉。开启后需让
Claude Code 完成至少一次响应，才能产生新的额度缓存；缓存首次出现或发生变化
时会立即刷新，不必等待常规五分钟轮询。

这里的 Claude 额度来源严格属于 **Claude Code 终端的 `statusLine`**。Claude
桌面客户端虽然可能运行本地 Agent 并产生 Hook 任务事件，但它不渲染终端
status line，因此桌面客户端里的多轮对话不会刷新这份额度缓存。界面显示的
“N 分钟前”是最后一次真实额度采样时间，不会拿普通对话事件伪装成额度更新。

Codex 额度读取是只读、版本门禁的实验能力，当前只接受已固化 fixture 的桌面
内核 0.144.2、CLI 0.144.4 和 CLI 0.144.5 rollout 结构，并只投影 7 天
（10080 分钟）窗口到“本周”。
数据缺失、过期或格式不兼容时，界面会显示具体窗口“不可用”，不会虚构百分比。

## 9. 本地数据与导出

默认数据目录：

```text
~/.flow-agent/
```

其中包含私有 Socket、SQLite 数据、缓存、安装备份和稳定 Hook 帮助程序。
运行目录使用当前用户私有权限；Flow Agent 没有遥测、云后端或自动出站上报。

### 完整本地备份

包含本机 SQLite 中已经脱敏的各表：

```bash
./target/release/flow-agent export > flow-agent-backup.json
```

### 只导出聚合统计

不包含会话、事件、命令、项目或路径：

```bash
./target/release/flow-agent export-metrics > flow-agent-metrics.json
```

也可以在设置页点击“导出 JSON”或“导出统计”。统计不会自动上传；是否分享完全
由用户决定。

### 彻底清除运行数据

在设置页点击“彻底清除”，并输入大写 `DELETE` 确认。该操作删除本地事件、
会话、额度缓存、设置和诊断数据，但保留 Provider Hook 接入和安装备份，避免
把 Provider 配置留在半安装状态。

## 10. 诊断模式

诊断采集默认关闭。只有排查问题时才临时开启：

```bash
./target/release/flow-agent diagnostics enable --minutes 10
./target/release/flow-agent diagnostics status
```

允许时长是 1–60 分钟。诊断文件只记录固定事件类别、Provider、采集时间、
是否需要回复和 payload 字节数，不记录原始 Hook 内容、session、路径、prompt、
命令、参数、URL 或 token；单文件最大 1 MiB，到期自动删除。

问题复现后立即清除：

```bash
./target/release/flow-agent diagnostics clear
```

## 11. 自检与故障排查

先运行：

```bash
./target/release/flow-agent doctor
```

需要保存机器可读报告时：

```bash
./target/release/flow-agent doctor --json > flow-agent-doctor.json
```

### 提示“Provider CLI 未安装”

确认 `claude --version` 或 `codex --version` 在同一个终端可用，然后重新执行
安装。Flow Agent 不会为不存在的 Provider 创建空配置。

### Hook 已安装但一直显示未接入

- 启动一个安装后的全新 Provider 会话；
- Codex 运行 `/hooks` 并确认已信任；
- 保持 Runtime 运行；
- 回到首次运行窗口点击“刷新状态”；
- 运行 `flow-agent doctor` 查看配置、信任、Runtime 和 fail-open 探针结果。

### 页面显示“Runtime 未连接”

这表示本机控制页无法再连接 Runtime，不等同于 Codex Hook 未安装。按以下顺序
处理：

1. 检查启动 Runtime 的终端是否仍在运行；关闭终端、按过 `Control-C` 或电脑
   重启都会停止当前 v1 Runtime；
2. 关闭旧 Flow Agent 浏览器页面；
3. 在仓库之外也可以直接运行稳定程序：

   ```bash
   ~/.flow-agent/bin/flow-agent serve --open
   ```

4. 保持该终端开启，只使用本次命令自动打开的新页面；
5. 在另一个终端运行：

   ```bash
   ~/.flow-agent/bin/flow-agent doctor
   ```

6. 只有 `runtime.control_loop` 通过后，再排查 Codex `/hooks` 信任和真实新会话。

如果启动时报“已有实例”，说明另一个 Runtime 已持有单实例锁。优先找到原启动
终端并继续使用它打开的页面；需要重启时，在原终端按 `Control-C`，确认当前没有
等待中的批准，再重新执行 `serve --open`。

### 权限卡消失或显示已过期

回到原 Provider 终端处理。不要重复提交旧卡片；Flow Agent 会拒绝向已经失效
的 waiter 写入决定。

### 浏览器窗口被关闭

停止原 Runtime，再重新运行 `flow-agent serve --open`。SQLite 中的会话和事件
会恢复；重启前尚未完成的批准会诚实标为过期，不会伪造仍可控制的卡片。

### Runtime 崩溃

Provider Hook 会自动交还终端。重新启动 Runtime 后，再启动一个新的 Provider
会话；不要依赖崩溃前仍在等待的旧权限卡。

## 12. 卸载 Hook

只移除 Flow Agent 自己安装的条目，保留其他 Hook 和未知配置：

```bash
./target/release/flow-agent uninstall-hooks claude
./target/release/flow-agent uninstall-hooks codex
# 或同时卸载
./target/release/flow-agent uninstall-hooks all
```

卸载后运行 `flow-agent doctor` 核对结果。卸载 Hook 不等同于删除本地运行数据；
如需清除数据，再使用设置页的 `DELETE` 操作。

## 13. 测试候选版建议验收清单

本地测试时建议逐项确认：

1. `serve --open` 能打开控制界面；
2. Claude 安装后新会话产生真实事件；
3. Codex `/hooks` 信任后新会话产生真实事件；
4. Claude 和 Codex 各完成一次“允许”；
5. 各完成一次“拒绝”；
6. 点击允许/拒绝后在 3 秒内成功撤回；
7. “去终端处理”后原终端可以继续操作；
8. 停止 Runtime 后 Provider 仍能回到原生流程；
9. 设置、通知、额度不可用态和统计导出符合实际数据；
10. 重启 Runtime 后历史状态恢复，旧权限请求不会伪装成仍可控制。

发现问题时，请记录：系统版本、Flow Agent 提交 SHA、Claude/Codex 版本、
`flow-agent doctor --json` 的脱敏输出和最短复现步骤。不要公开提交 prompt、完整
命令、源码、token、个人路径或原始 Provider 对话。
