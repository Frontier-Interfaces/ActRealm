# Flow Agent v1 中文使用教程

本教程面向准备从 GitHub 源码安装和测试 Flow Agent v1 的用户。当前 v1
优先验证 macOS arm64/x64，Claude Code 与 Codex 是正式支持的两个 Provider；
二者都可以使用命令行版或本机桌面客户端。界面运行在本机浏览器中，Runtime、
数据库和 Hook 通信都留在本机。

> 当前已提交的 `agent/v1-full` 分支功能实现到 M14，实时用量、上下文、
> 估算 API 价和 Claude OAuth 额度已完成本机验收。M13 真实 Provider 最终复验和连续 48 小时发布门禁
> 仍未完成，因此不应称为最终 v1 Release。实时状态见
> [STATUS.md](STATUS.md)。

## 1. 使用前准备

需要：

- macOS；
- Git；
- Rust stable 1.85 或更高版本（`rustc --version`）；
- 至少安装一种 Provider：Claude Code CLI、Claude Desktop、Codex CLI 或
  ChatGPT/Codex 桌面客户端。只使用桌面客户端时，不要求 `claude` 或 `codex`
  出现在终端 `PATH` 中。

Flow Agent 不会替你安装、启动或拥有 Claude/Codex 会话。它只接收 Provider
官方 Hook 事件，并在 Provider 发出权限请求时提供允许、拒绝或交还终端三种
操作。

### 1.1 终端与客户端支持矩阵

是否有数据不取决于界面长得像终端还是客户端，而取决于该会话是否在本机执行
已经安装并信任的 Hook：

| Provider 运行形态 | v1 状态 | 说明 |
| --- | --- | --- |
| Claude Code CLI（终端） | 已验证 | 当前 P0 合约与真实版本测试形态 |
| Claude Code Desktop 的 Local 会话 | 支持接入 | 安装器直接识别 `Claude.app` 并合并共享的 hooks/settings，不要求全局 Claude CLI |
| Claude Code 远程/云端会话 | 不支持本机控制 | Hook 不在本机时无法连接本机 Unix Socket |
| Codex CLI（终端） | 已验证 | 当前 P0 合约、信任流程与真实版本测试形态 |
| ChatGPT/Codex 桌面客户端的本地任务 | 支持接入 | 安装器识别桌面 App 及其内置 Codex；Hook 仍须用户用内置 Codex 完成 `/hooks` 信任 |
| Codex 云端/Web 任务 | 不支持本机控制 | 云端任务无法连接本机 Runtime |

因此“Claude Desktop + Codex 客户端”也不要求额外安装两个全局 CLI：Flow Agent
直接写入各自的用户级 Hook 配置。本机任务加载并信任 Hook 后即可产生数据；
远程/云端任务仍无法连接本机 Runtime。

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
> Codex 会话。若终端没有全局 `codex` 命令，运行安装结果或首次接入卡片给出的
> 内置 Codex 路径（标准安装通常是
> `'/Applications/ChatGPT.app/Contents/Resources/codex'`），然后输入 `/hooks`，
> 逐项检查并亲自信任 Flow Agent；Agent 不得绕过或代替用户完成信任。信任后
> 再次启动全新 Codex 会话，并在另一个终端执行
> `~/.flow-agent/bin/flow-agent doctor`。只有 `runtime.control_loop` 通过、Codex
> Hook 配置/信任检查通过、且面板收到该新会话的真实事件，才允许报告安装成功。
> 如果网页显示“Runtime 未连接”，先判断 `serve --open` 终端是否仍在运行；若已
> 退出，关闭旧网页并重新运行 `~/.flow-agent/bin/flow-agent serve --open`，使用
> 它新打开的页面继续，不能把问题归因于网页文件或仅重复修改 Hook。

如果目标机器还需要 Claude，只要 Claude CLI 或 `/Applications/Claude.app` 任一
存在，就可额外执行：

```bash
~/.flow-agent/bin/flow-agent install-hooks claude
```

不要为机器上完全不存在的 Provider 创建空配置；桌面客户端本身属于有效的
Provider 安装，不需要再补装同名 CLI。

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

## 4. 接入 Claude Code CLI 或 Desktop

### 界面方式（推荐）

1. 在首次运行窗口找到 Claude 卡片；
2. 点击“安全接入”；
3. Flow Agent 会先备份，再语义合并 `~/.claude/settings.json`；
4. 重新启动一次真实的本机 Claude Code CLI 或 Desktop 会话；
5. 回到 Flow Agent 点击“刷新状态”；
6. 只有收到安装后的真实事件，状态才会变成“已接入”。

### 命令行方式

```bash
./target/release/flow-agent install-hooks claude
./target/release/flow-agent doctor
```

安装器只添加 Flow Agent 自己的 Hook，不删除用户原有 Hook，也不整份重写
未知配置。安装器会接受 `PATH` 中的 Claude CLI 或标准位置的 `Claude.app`；两者
都不存在时才会拒绝创建配置。

## 5. 接入 Codex CLI 或桌面客户端

Codex 比 Claude 多一个必须由用户亲自完成的信任步骤。

### 界面方式（推荐）

1. 在首次运行窗口点击 Codex 的“安全接入”；
2. 打开一个新的 Codex 会话；若没有全局 CLI，按卡片显示的路径在终端启动桌面
   App 内置的 Codex；
3. 在 Codex 中输入 `/hooks`；
4. 逐项检查 Flow Agent 命令并选择信任；
5. 重新启动一个 Codex 会话；
6. 回到 Flow Agent 点击“刷新状态”，等待真实事件验证。

Flow Agent 不会修改 Codex 的信任状态，也不会绕过 `/hooks` 审查。如果 Hook
定义在升级后发生变化，Codex 可能要求重新信任。

标准版 ChatGPT 桌面客户端的内置命令通常位于：

```bash
'/Applications/ChatGPT.app/Contents/Resources/codex'
```

它只用于打开官方的 `/hooks` 审查界面，不是另装一个 Codex CLI。若 App 安装在
其他受支持位置，以 Flow Agent 接入卡片的“内置 Codex”命令为准。

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
2. 像平常一样启动本机 Claude/Codex CLI 或桌面客户端任务；
3. Agent 的客户端会话标题、当前任务、模型、实时状态和需要关注的事件会出现在 Flow Agent；
4. Provider 发出权限请求时，待处理区域会按真实能力显示“可在 Flow Agent
   审批”或“原界面请求批准、仅同步状态”。

“Agent 任务”只展示以下会话：仍在运行、仍有待处理事项，或最后一次活动距今
不超过 30 分钟。结束且超过 30 分钟的会话仍可按数据保留设置存在本机历史中，
但不会继续占据主列表。主标题优先来自 Claude/Codex 客户端自己的会话标题；
下一行直接显示最近一次用户任务的限长摘要，不添加“当前任务”等前缀；再下一行
只显示当前模型。不会用用户名、项目名或 Provider 名冒充标题。Claude/Codex 行
使用对应图像标识，不再显示 `Cl` / `Co` 字母占位。活动行会按真实事件
显示思考计时、正在运行的工具、等待批准、完成、失败或空闲，缺少工具级 Hook
时只诚实显示轮级状态。

待处理卡片上的“在 Agent 任务中查看”会选择对应会话，将它置顶、高亮并滚动到
可见位置；即使该会话已经超过 30 分钟，只要仍有待处理事项也不会被过滤掉。

权限请求分成两类，按钮不能混用：

1. **可在 Flow Agent 审批**：Hook/Connector 提供了本次请求的实时 reply
   channel 和 requestId。只有这类卡片支持下面的允许、拒绝、撤回和交回原界面。
2. **原界面请求批准，Flow Agent 仅同步状态**：批准界面属于 Claude/Codex
   原生客户端，Flow Agent 没有本次批准的回复通道。这类卡片只提供“去 Agent
   处理”“待会提醒”“忽略”，绝不显示假的允许/拒绝按钮。

可回复权限卡支持：

- **允许**：向 Provider 写回本次请求的允许结果；
- **拒绝**：向 Provider 写回本次请求的拒绝结果；
- **撤回**：允许/拒绝点击后有 3 秒提交等待，在写回前可以撤回；
- **去终端处理**：Flow Agent 不做决定，立即把本次请求交还 Provider 原生流程。

允许和拒绝只控制当前 `PermissionRequest`，不是永久授权。Flow Agent 不实现
“始终允许”，也不会绕过 Provider 策略、企业规则或沙箱。

当用户、Provider 自动审查或 Agent 自己在原界面处理 native approval 后，Flow
Agent 只在收到匹配的 `PostToolUse`、Thread 状态清除或其他可靠终态时消退对应待
处理、关闭通知并取消任务卡的“原界面等你”。消退只代表“不再等待”，不能证明
用户批准、拒绝或命令已经执行。普通 running/tool 更新也不能把仍在等待的原生批准
错误覆盖成“在跑”。

Agent 提问支持：

- Claude `AskUserQuestion` 会在待处理卡直接显示一到四个问题，支持单选、多选和
  “其他答案”；提交后使用 Claude 官方 `updatedInput.answers` 回复；
- Claude `Elicitation` 会显示经过校验的本地表单，支持提交、拒绝和取消；
- 标记为 secret/password 的字段使用密码输入框。答案只存在于当前网页表单、
  认证后的 localhost 请求、Runtime 内存 waiter 和一次 Provider 回复中，不写入
  SQLite、日志、诊断或导出；
- 选择“去 Agent 回答”会让 Claude Hook 保持 stdout 为空并回到原生界面；
- 普通 Codex Hook 会话不会显示假的回答框。只有任务卡明确显示“app-server
  托管，可控制”时，Codex `requestUserInput` 才能在待处理卡中回答。

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
- 任务卡“简洁 / 详细 / 开发者”三档，以及项目、任务、模型、活动、计划、
  Token、上下文、估算 API 价、工具、权限模式、子 Agent、环境、恢复/控制状态和
  开发者 ID 等字段。

字段选择器只接受服务端安全目录中的结构化字段。原始 Hook Payload、完整命令、
文件内容和 transcript 不会作为可选项，也不能通过手工设置 API 强行开启。任务卡
“详情”抽屉使用相同白名单。

额度模块不再固定为三项。Flow Agent 会展示额度来源实际返回的全部有效窗口：
例如 Claude 5 小时、7 天或额外命名额度，以及 Codex 5 小时、7 天、月度或未来
新增的日额度。窗口名称和周期来自真实结构，不会把所有 Codex 账户都写成“本周”。

M14 起，Claude 额度优先尝试 Anthropic 的官方 OAuth usage 接口。只要本机已有
Claude 登录凭据，Claude 桌面端或 Claude Code 的额度都可以在后台刷新，不再要求
必须先运行一轮 Claude Code CLI。Flow Agent 每分钟最多启动一次后台刷新；请求
期间不会卡住页面。OAuth Token 只在进程内存中短暂存在，通过 stdin 交给系统
`curl`，不会出现在命令行参数、SQLite、缓存、日志、诊断或导出中。

如果 OAuth 凭据不可用、接口限流或断网，Flow Agent 会继续显示最后一次经过校验
的值，并回退到 status-line 额度桥。Claude 没有自定义 `statusLine` 时可直接开启
额度桥；已有自定义
`statusLine` 时，设置页会提供明确的“保留现有并开启”。只有点击该动作后，Flow
Agent 才会备份完整原对象、安装代理并继续显示原 status-line 输出；卸载额度桥
会把原对象逐字段恢复。不会静默覆盖，也不会把已有脚本的输出吞掉。开启后需让
Claude Code 完成至少一次响应，才能产生新的额度缓存；缓存首次出现或发生变化
时会立即刷新，不必等待常规五分钟轮询。

界面会标明 `OAuth 自动同步`、`Claude 对话同步` 或 `本机 Session 同步`。
“N 分钟前”始终是最后一次真实采样时间，不会拿普通对话事件伪装成额度更新；
超过 30 分钟也不会清空或改写真实百分比。OAuth 不可用时，Claude 桌面客户端
本身不会触发终端 status line，此时只有下一次 Claude Code status-line 响应才能
产生新的回退样本。

Codex 额度读取是只读的实验能力。适配器在有限大小的 rollout 尾部查找并严格
校验 `rate_limits` 数值结构，展示 primary/secondary 中所有有效周期；不再绑定
0.144.x 的具体补丁版本，也不再只保留 10080 分钟窗口。数据缺失或格式不兼容
时显示“不可用”，已成功采样但暂时没有新记录时保留最后有效值。

## 9. 任务标题、时间与跳回原对话

- 大标题是 Provider 自己的会话标题：Claude 先接收官方 Hook 的
  `session_title`，再只读识别本地 JSONL 中的用户自定义标题和 AI 标题；Codex
  只读本机 `~/.codex/session_index.jsonl` 中该会话最新的 `thread_name`。没有
  可靠标题时才回退为本轮问题的最多 64 字摘要，不再把项目名放在主标题。
- 第二行直接显示本轮问题内容，例如 `你做吧`，不添加“当前：”或“当前任务”文字。
  第三行只显示当前调用模型，例如 `gpt-5.6-sol`；不混入 Provider、项目名、标题
  来源或 Token 信息。
- Provider 会话标题和本轮问题仍是两个独立字段，既与客户端标题一致，也不会
  丢掉 Agent 正在做的事。
  Flow Agent 最多每 2 秒检查一次最近 30 分钟会话的本地标题变化；无需重启。
- 标题读取有大小上限，只持久化规范化后的标题和来源；不会把 transcript 内容或
  transcript 路径发到浏览器。Claude 官方标题不会被旧 AI 标题倒退覆盖，后续
  用户自定义标题仍可更新。不会显示用户名充当任务名。
- 时间以本轮从开始到结束的总时长为主，运行中同时显示当前阶段时长。
- Token 来自本机 Claude transcript 或 Codex rollout 的结构化 usage，约 1 秒刷新。
  卡片中的“会话累计 Token”与“本轮 Token”是两个字段；缓存 Token 不重复计入
  Codex input，推理 Token 不重复计入 output。
- 上下文显示的是当前一轮占用：Claude 优先使用官方 StatusLine
  `current_usage/context_window_size`，Codex 使用 `last_token_usage` 与 Provider
  context window。绝不再用会话累计 Token 除以上下文窗口冒充实时占用。
- “估算 API 价”优先使用 Provider 官方会话费用字段，否则使用带日期的本地公开
  API 单价快照；它只是横向比较值，不是 Claude/Codex 订阅账单。未知模型不显示
  价格，不用零元补位。
- 待处理在原 Agent 中被批准、拒绝或结束后，会自动从待处理区消退，并同步取消
  任务卡的“等你”。“忽略”会把非授权提醒隐藏；授权忽略会先安全交回原 Agent。
- 点击任务卡会按实际能力执行，并在卡片上明确写出：`精确打开对话`、
  `打开对应终端`、`只能打开应用` 或 `当前环境不支持跳转`。Codex 桌面会使用
  官方 `codex://threads/<thread-id>`；Terminal/iTerm 只有拿到可验证的标签页标识
  才声称可以精确定位。
- Runtime 重启后会从 SQLite 恢复仍处于运行中的会话、当前轮次开始时间和跳转
  能力。窗口 ID、TTY 和 Bundle ID 只在本地用于跳转，不会发送到浏览器快照。

### 9.1 恢复状态和 Codex app-server Connector

任务卡会明确显示以下恢复状态之一：

- `已重新连接，可控制`：该 Codex Thread 已通过官方 app-server `thread/resume`
  重连，可在面板回答 `requestUserInput`；
- `仍在运行，仅可观察`：外部 Hook 会话对应进程仍存活，但 Flow Agent 不拥有会话；
- `历史已恢复，等待新事件`：数据库记录已恢复，尚无进程或 Provider 新事件确认；
- `已失去控制`：先前记录的 Provider 进程已不存在；
- `已结束`：本轮或会话已经结束。

Runtime 会用桌面内置或 PATH 中的官方 Codex，在 `~/.flow-agent/run/` 私有 Unix
Socket 上维护持久 app-server，再通过官方 proxy 连接；不要求 standalone 专用的
`app-server daemon start`。未托管的 Codex 任务卡仅在
Connector 版本兼容时显示“启用可控制连接”；用户点击后才调用 `thread/resume`
并持久化该 Thread ID。Flow Agent 重启后会重新 `thread/list` 并恢复这些显式托管
Thread。若当前 Codex 版本不支持 daemon/proxy 或实验性 `requestUserInput`，设置页
会显示 Connector 不可用，原来的 Hook 观察和权限控制仍可使用。

“恢复展示”不等于恢复旧回复通道：重启前尚未提交的权限和问题 waiter 一律过期，
断开的 Hook stdout/RPC 连接不会被伪造。Provider 在重连后发出新的问题时才会创建
新的可回答卡片。

## 10. 本地数据与导出

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

## 11. 诊断模式

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

## 12. 自检与故障排查

先运行：

```bash
./target/release/flow-agent doctor
```

需要保存机器可读报告时：

```bash
./target/release/flow-agent doctor --json > flow-agent-doctor.json
```

### 提示“未找到客户端”

确认相应 CLI 可从当前终端运行，或桌面 App 位于标准位置：
`/Applications/Claude.app`、`/Applications/ChatGPT.app` 或用户目录的
`~/Applications`。然后重新启动 Runtime 并刷新接入状态。Flow Agent 不会为
完全不存在的 Provider 创建空配置，但不再强制桌面用户安装全局 CLI。

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
会恢复；重启前尚未完成的批准和问题会诚实标为过期，不会伪造仍可控制的卡片。

### Runtime 崩溃

Provider Hook 会自动交还终端。重新启动 Runtime 后，再启动一个新的 Provider
会话；不要依赖崩溃前仍在等待的旧权限卡。

## 13. 卸载 Hook

只移除 Flow Agent 自己安装的条目，保留其他 Hook 和未知配置：

```bash
./target/release/flow-agent uninstall-hooks claude
./target/release/flow-agent uninstall-hooks codex
# 或同时卸载
./target/release/flow-agent uninstall-hooks all
```

卸载后运行 `flow-agent doctor` 核对结果。卸载 Hook 不等同于删除本地运行数据；
如需清除数据，再使用设置页的 `DELETE` 操作。

## 14. 测试候选版建议验收清单

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
11. Claude/Codex 已有会话标题时，卡片主标题与客户端一致；重命名后最多约 2 秒
    更新，第二行直接显示任务内容，第三行只显示当前模型。
12. 设置任务卡为简洁/详细/开发者档，刷新页面后字段选择仍保留，详情中没有原始
    Hook JSON、完整命令或 transcript。
13. 触发 Claude AskUserQuestion 和 Elicitation；从面板回答后 Agent 正常继续，
    password 字段不出现在导出中，过期问题不能重复提交。
14. 对支持的 Codex 会话点击“启用可控制连接”，确认状态变为“已重新连接，可控制”，
    `requestUserInput` 可回答；重启 Runtime 后 Thread 再次恢复。未托管 Codex 会话
    始终显示“仅可观察”，不得出现假回答框。
15. 在 Codex/Claude 原界面触发 native approval，确认 Flow Agent 显示“原界面
    请求批准 / 仅同步状态”，没有允许/拒绝按钮，任务卡显示“原界面等你”。
16. 分别在原界面批准和拒绝上述请求，确认可靠 Provider 终态到达后待处理与通知
    消退、任务卡取消等待；Flow Agent 只显示中性“已在原界面处理”，不得写成
    “已批准”“已拒绝”或“已执行”。该两向真实 Provider 复验是 M13 尚待完成的
    手工验收项。

发现问题时，请记录：系统版本、Flow Agent 提交 SHA、Claude/Codex 版本、
`flow-agent doctor --json` 的脱敏输出和最短复现步骤。不要公开提交 prompt、完整
命令、源码、token、个人路径或原始 Provider 对话。
