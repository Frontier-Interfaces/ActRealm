# ActRealm H6-B opaque push 与移动端配置

日期：2026-08-21

分支：`actrealm最新版`

状态：H6-B 代码门通过；开发 Firebase iOS App 已注册；提交
`596d68816510b53ef2e169e718966b865af904de` 的 Functions、Rules 与 indexes 已部署到
`actrealm-share-dev`。尚未创建或上传 APNs 私钥、启用 production App Check enforcement，且
本机没有已登记的真实 iPhone/Watch，所以 H6 真实设备门和公开远程 approve 仍未通过。

## 本次完成

### Cloud

- 新增 `registerPushEndpoint` / `revokePushEndpoint` callable；
- 只有正式 Firebase 身份、有效设备 proof、不可变 `iOS` platform 可以登记；macOS、watchOS、
  匿名身份和错误 credential 全部 fail closed；
- 原始 FCM token 只存 `remotePushEndpoints` 服务端私有集合，客户端 Rules 全拒绝，设备列表和
  callable 回包都不返回 token；
- endpoint 以 UID + device ID 的 SHA-256 派生 ID 去重，token 刷新原位轮换，90 天未登记由 TTL
  清理；设备撤销立即删除 endpoint；
- 新 envelope 只发送一次通知，heartbeat 重发不会制造通知风暴；每用户每小时最多 120 次
  push，批次瞬时错误使用 250 ms / 1 s 有限退避；永久失效 token 自动删除；
- push 失败与重试失败都不修改审批状态，只记录不含 token 的 endpoint ID、envelope ID 和错误码。

### Opaque notification

通知 data 固定只有：

- `schemaVersion`；
- `envelopeID`；
- `hostDeviceID`；
- `categoryID`；
- `collapseID`；
- `expiresAtMillis`。

锁屏标题固定为 `ActRealm`，正文固定为“ActRealm 有一项操作需要审核”。provider、risk、
operation category、command shape、Prompt、路径、文件、Token、费用、Runtime credential 和
Provider reply channel 均不进入 notification。APNs 使用 envelope collapse ID、host thread ID、
服务端 expiry 和 `content-available`；push 只是唤醒提示，不是事实源。

### iPhone 与 Watch

- Firebase Apple SDK 继续精确固定在 `12.17.0`，新增 `FirebaseMessaging`；
- 已注册独立开发 Firebase iOS App：
  `1:226935607548:ios:a14e12792480213b050779`，bundle ID 为
  `com.frontierinterfaces.actrealm.mobile`；
- XcodeGen 的 source-of-truth 已包含公开 iOS Firebase config、后台 remote notification、开发
  APNs、Sign in with Apple 和开发 App Attest entitlement，重复生成不会覆盖丢失；
- 禁用 Firebase AppDelegate swizzling，显式登记 APNs token、接收/轮换 FCM token、处理前台、
  后台和用户点击；
- 收到 notification 后只校验 live opaque routing facts，随后重新认证调用
  `listRemoteApprovals`，不把 payload 当成风险或状态；App 启动和前台运行仍主动轮询；
- 通知设置页解释锁屏隐私、事实重新拉取、系统通知设置和手动重新登记；
- Watch 不链接 Firebase、不保存 iPhone token、不直接调用 Cloud。iPhone 拉取后通过
  WatchConnectivity 发布最新脱敏 projection，系统可转发同一条通用 iPhone 通知；Watch 直接
  approve 继续关闭。

## 验证

- Functions TypeScript：41/41；新增冻结 payload、通用文案、敏感字段缺失和后台唤醒断言；
- Firestore Rules：85/85；新增 endpoint 读写全拒绝；
- Auth + Functions + Firestore 完整 Emulator：通过；新增正式/匿名身份、iPhone/Watch platform、
  credential、登记、重复轮换、撤销、私有存储和审批发布链；
- iOS source-check：通过；
- iOS 26.5 Simulator：4/4；新增 live/expired opaque wake-up；
- Watch arm64 device SDK：通过；
- `plutil`、Firebase app list、`git diff --check` 和仓库私钥模式扫描：通过。
- 仓库提交门禁：Rust fmt、零 warning clippy、workspace tests、offline release build、语言合同及
  UTC macOS 全套通过；macOS ShareProjection 35/35、Swift Testing 192/192。

Apple Development 真机签名命令已执行，但 Xcode 明确报告团队当前没有已登记设备，无法生成
`com.frontierinterfaces.actrealm.mobile` 与 Watch companion 的 development provisioning
profile。iPhone scheme 还需要 watchOS 26.5 Simulator runtime；当前机器只有 iOS 26.5 runtime。
这两项是环境门，不被记录成代码通过。

## 明确未完成

- 未创建、下载、上传或提交任何 APNs authentication key；
- 未在 Firebase Cloud Messaging 配置 APNs；
- 开发 Functions、Rules 与 indexes 已部署；未部署任何 production 项目；
- App Check 保持 monitor，未切换 production enforcement；
- 未在真实 iPhone/Watch 验证 push 到达、锁屏文案、网络切换、撤销、竞态或 Runtime apply；
- 未完成 H6 15.15 的 iPhone 20 + Watch 10 请求矩阵；
- 未打开 Watch low/medium approve；
- 未声明 internal beta、生产或公开发布就绪。

## 下一道门

1. Apple Developer 页面已验证当前账号无权创建 Key，并明确要求联系 Team Admin；由 Team Admin
   创建 APNs Key 或为当前账号开放对应权限后，才能下载并上传到 Firebase。私钥只进入
   Apple/Firebase 受控配置，不进仓库；
2. 连接并登记真实 iPhone/Watch，或安装 watchOS 26.5 Simulator runtime 仅做 UI 补充验证；
3. 执行 iPhone 20 + Watch 10、Codex/Claude、离线/睡眠、网络切换、多设备竞态、撤销/重配矩阵；
4. 零高风险 approve、零重复 Provider reply、零过期执行、零假 confirmation 后，才讨论扩大测试；
5. production App Check enforcement、生产 APNs、App Store/TestFlight 和公开发布继续单独审批。

## 开发部署记录

- 用户明确允许 Firebase CLI `--force` 计费确认；
- Firestore Rules 与 indexes 部署成功；
- Functions 全量部署成功；Cloud Functions 一度触发区域 mutation 429，CLI 等待后自动重试成功，
  最终没有失败项；
- `bootstrapStatus`、`registerPushEndpoint`、`revokePushEndpoint`、
  `publishRemoteApprovalEnvelope` 与 `listRemoteApprovals` 均为 Node.js 22、`ACTIVE`、min 0、max 2；
- 同一次部署 hash 为 `0e229c55b70e6dc1ba23201bbd78d877233b1983`；
- `ACTREALM_ENFORCE_APP_CHECK` 明确未设置，开发部署保持 monitor；
- Apple Developer 创建页返回“当前账号不允许执行此操作，请联系 Team Admin”，没有产生 `.p8`
  下载，也没有向 Firebase 上传任何 credential。

## 暂停检查点

用户于 2026-08-21 决定先暂停 H6。下一次必须从 Apple Team Admin 权限与 APNs 配置继续，不得
重复 H6-A/H6-B 代码、Firebase iOS App 注册或开发 Cloud 部署。完整完成项、未完成项、Git/Cloud
锚点和恢复步骤见 `ACTREALM_H6_PAUSE_HANDOFF_2026-08-21.md`。
