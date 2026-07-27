# ActRealm 第 9 项：备份治理与可复现 CI 验证报告

- 日期：2026-07-27（Asia/Shanghai）
- 基线：`1dff02d879443876a1ab59aca1654ca9d084e7ed`
- 工作树：本地隔离工作树，尚未 commit、push 或安装候选版本
- 范围：显式备份治理、macOS/Web 设置入口、CI 固定、历史证据脱敏

## 已完成

1. 备份文件名加入来源身份，Claude 设置、Codex hooks、Codex config 与安装状态即使同名也不会混淆。
2. 备份目录和文件分别强制为 `0700` 与 `0600`，写入仍采用原子替换。
3. 新增备份数量/总大小查询；macOS 与 Web 设置页均展示真实统计。
4. 新增显式删除操作，必须输入 `DELETE BACKUPS`。不会自动轮转或自动删除。
5. 删除前会完整检查目录；遇到符号链接、公开权限、非普通文件或非 ActRealm 文件即拒绝，不删除任何已检查文件。
6. 所有 GitHub Actions 固定到完整提交 SHA；Rust 固定为 `1.97`，Xcode 固定为 `26.6`，cargo-audit 固定为 `0.22.2 --locked`。
7. 新增 `scripts/check-ci-pins.sh`，阻止可变 Action 标签、`stable/latest-stable`、未固定的 cargo install、远程管道安装和 Intel 目标进入工作流。
8. 删除仓库当前树中的原始截图、崩溃报告、进程列表、请求 JSON、运行日志和压力脚本；保留不含个人内容的脱敏结论索引，并阻止未来原始证据被跟踪。

## 验证结果

- `cargo test -p actrealm-installer --offline`：通过；16 个安装器集成测试、4 个 statusline 测试。
- `cargo test -p actrealm-server --offline`：通过；27 个单元测试、7 个 API 测试、3 个性能测试；2 个手工预览按设计忽略。
- `./scripts/check-ci-pins.sh`：通过。
- `./scripts/check-actrealm-language.sh`：通过；Runtime 合约 55 条消息、58 个 API 错误、55 个已发出消息代码。
- `apps/macos/Scripts/test.sh`：通过；23 个 Suite、129 项测试。
- `git diff --check`：通过。

## 边界

- 没有自动删除任何用户备份。
- 没有新增 Intel 支持、自动更新、48 小时浸泡或无障碍工作。
- 历史远端 Git 记录没有重写；这里只清理当前工作树。
- 第 10 项仍需完成文档、全量发布门禁、性能/安全专项与本地候选安装验收。
