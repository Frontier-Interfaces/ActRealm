import AppKit
import AVFoundation
import ActRealmKit
import SwiftUI
import UniformTypeIdentifiers

public enum SettingsSection: String, CaseIterable, Hashable, Identifiable, Sendable {
    case general
    case agents
    case notifications
    case theme
    case display
    case data

    public var id: Self { self }

    static var visibleCases: [Self] { allCases }

    var title: String {
        switch self {
        case .general: "通用"
        case .agents: "settings.tab.agents"
        case .notifications: "通知"
        case .theme: "主题"
        case .display: "显示"
        case .data: "数据"
        }
    }

}

public struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering
    @State private var selection: SettingsSection

    public init(
        initialSection: SettingsSection = .general
    ) {
        _selection = State(initialValue: initialSection)
    }

    public var body: some View {
        HStack(spacing: 0) {
            if snapshotRendering {
                snapshotSidebar
            } else {
                List(SettingsSection.visibleCases, selection: $selection) { section in
                    Text(LocalizedStringKey(section.title))
                        .tag(section)
                }
                .listStyle(.sidebar)
                .frame(width: 168)
            }

            Divider()

            detail
                .id(selection)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(
            width: 920,
            height: 660
        )
        .background(Color(nsColor: .windowBackgroundColor))
        .overlay(alignment: .bottom) {
            VStack(spacing: 8) {
                if let error = model.settingsSaveError {
                    SettingsSaveFeedback(message: error, isError: true) {
                        model.retrySettingsSave()
                    }
                } else if let notice = model.settingsSaveNotice {
                    SettingsSaveFeedback(message: notice, isError: false)
                }

                if let toast = model.toastMessage {
                    StatusToast(text: toast)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                        .allowsHitTesting(false)
                }
            }
            .padding(.horizontal, 24)
            .padding(.bottom, 16)
        }
        .animation(.easeOut(duration: 0.22), value: model.toastMessage)
        .animation(.easeOut(duration: 0.22), value: model.settingsSaveError)
        .animation(.easeOut(duration: 0.22), value: model.settingsSaveNotice)
        .onAppear { model.setSettingsVisible(true) }
        .onDisappear { model.setSettingsVisible(false) }
        .task {
            model.refreshRuntimeDiagnostics()
            await model.refreshSettings()
            await model.refreshSetup()
            if ProductScope.companionManagementEnabled {
                await model.refreshCompanionConnections()
            }
        }
    }

    private var snapshotSidebar: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(SettingsSection.visibleCases) { section in
                Text(LocalizedStringKey(section.title))
                    .font(.system(size: 12.5, weight: section == selection ? .semibold : .regular))
                    .foregroundStyle(section == selection ? Color.accentColor : Color.primary)
                    .padding(.horizontal, 10)
                    .frame(maxWidth: .infinity, minHeight: 28, alignment: .leading)
                    .background(
                        section == selection ? Color.accentColor.opacity(0.13) : Color.clear,
                        in: RoundedRectangle(cornerRadius: 7, style: .continuous)
                    )
            }
            Spacer()
        }
        .padding(10)
        .frame(width: 168)
        .background(Color(nsColor: .underPageBackgroundColor))
    }

    @ViewBuilder
    private var detail: some View {
        switch selection {
        case .general:
            GeneralSettingsPage()
        case .agents:
            AgentSettingsPage()
        case .notifications:
            NotificationSettingsPage()
        case .theme:
            ThemeSettingsPage()
        case .display:
            DisplaySettingsPage()
        case .data:
            DataSettingsPage()
        }
    }
}

private struct SettingsSaveFeedback: View {
    @Environment(\.locale) private var locale
    let message: String
    let isError: Bool
    var retry: (() -> Void)?

    init(message: String, isError: Bool, retry: (() -> Void)? = nil) {
        self.message = message
        self.isError = isError
        self.retry = retry
    }

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: isError ? "exclamationmark.triangle.fill" : "info.circle.fill")
            Text(localized(message, locale: locale))
                .font(.system(size: 11.5, weight: .semibold))
            Spacer(minLength: 8)
            if let retry {
                Button("重试", action: retry)
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
        }
        .foregroundStyle(isError ? Color.red : Color.primary)
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(isError ? Color.red.opacity(0.35) : Color.secondary.opacity(0.2))
        )
        .shadow(color: .black.opacity(0.14), radius: 12, y: 6)
        .frame(maxWidth: 620)
    }
}

private struct SettingsPageHeader: View {
    let title: String
    let subtitle: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(LocalizedStringKey(title))
                .font(.title2.weight(.bold))
            Text(LocalizedStringKey(subtitle))
                .font(.callout)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 24)
        .padding(.top, 22)
        .padding(.bottom, 10)
    }
}

private struct SettingsLabel: View {
    let title: String
    let detail: String?

    init(_ title: String, detail: String? = nil) {
        self.title = title
        self.detail = detail
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(LocalizedStringKey(title))
            if let detail {
                Text(LocalizedStringKey(detail))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct GeneralSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @State private var showingRuntimeMonitor = false

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "通用",
                subtitle: "查看本机服务状态并处理运行问题。"
            )
            Form {
                Section {
                    Picker(
                        selection: Binding(
                            get: { model.appLanguage },
                            set: { model.setAppLanguage($0) }
                        )
                    ) {
                        Text("跟随系统").tag(AppLanguage.system)
                        Text("简体中文").tag(AppLanguage.simplifiedChinese)
                        Text("English").tag(AppLanguage.english)
                    } label: {
                        SettingsLabel("界面语言")
                    }
                    .pickerStyle(.menu)
                } header: {
                    Text("语言")
                } footer: {
                    Text("默认跟随系统；界面立即切换，macOS 系统菜单在重启 ActRealm 后更新。")
                }

                Section {
                    LabeledContent {
                        Label(runtimeStatusTitle, systemImage: runtimeStatusSymbol)
                            .foregroundStyle(runtimeStatusColor)
                    } label: {
                        SettingsLabel("本机服务", detail: runtimeStatusDetail)
                    }

                    LabeledContent("最近同步") {
                        Text(model.lastSyncAt.map(ZhFormat.syncClock)
                            ?? localized("尚未同步", locale: locale))
                            .foregroundStyle(.secondary)
                    }

                    if let message = model.runtimeActionMessage {
                        Label(
                            localized(message, locale: locale),
                            systemImage: model.bridgeStatus.isListening
                                ? "checkmark.circle.fill"
                                : "exclamationmark.triangle.fill"
                        )
                        .foregroundStyle(model.bridgeStatus.isListening ? .green : .red)
                    }

                    HStack {
                        Button("诊断详情…") {
                            model.refreshRuntimeDiagnostics()
                            showingRuntimeMonitor = true
                        }
                        Button("重新检查") {
                            model.refreshRuntimeDiagnostics()
                        }
                        Spacer()
                        Button {
                            model.restartRuntime()
                        } label: {
                            if model.isRestartingRuntime {
                                ProgressView()
                                    .controlSize(.small)
                            } else {
                                Text("重启 Runtime")
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(model.isDemo || model.isRestartingRuntime)
                    }
                } header: {
                    Text("Runtime")
                } footer: {
                    Text("只有诊断详情会显示进程、锁和本机连接等技术信息。")
                }

            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
        .sheet(isPresented: $showingRuntimeMonitor) {
            RuntimeMonitorView().environmentObject(model)
        }
    }

    private var runtimeStatusTitle: String {
        if model.isRestartingRuntime {
            return localized("正在重启", locale: locale)
        }
        let key = switch model.bridgeStatus {
        case .listening: "运行正常"
        case .starting: "正在启动"
        case .absent: "未连接"
        }
        return localized(key, locale: locale)
    }

    private var runtimeStatusDetail: String {
        let key = switch model.bridgeStatus {
        case .listening: "Agent 事件与本机控制连接可用"
        case .starting: "正在等待本机服务完成启动"
        case .absent(let reason): reason ?? "本机服务暂时不可用"
        }
        return localized(key, locale: locale)
    }

    private var runtimeStatusSymbol: String {
        switch model.bridgeStatus {
        case .listening: "checkmark.circle.fill"
        case .starting: "clock.fill"
        case .absent: "exclamationmark.triangle.fill"
        }
    }

    private var runtimeStatusColor: Color {
        switch model.bridgeStatus {
        case .listening: .green
        case .starting: .orange
        case .absent: .red
        }
    }
}

private struct AgentSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "settings.tab.agents",
                subtitle: "管理 Claude Code、Codex 及可选的本机数据来源。"
            )
            Form {
                Section {
                    if let providers = model.setupInfo?.providers {
                        ForEach(providers) { provider in
                            providerRow(provider)
                        }
                    } else {
                        ProgressView("正在读取接入状态…")
                            .controlSize(.small)
                    }

                    HStack {
                        Spacer()
                        Button("刷新接入状态", systemImage: "arrow.clockwise") {
                            Task { await model.refreshSetup() }
                        }
                        .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
                    }
                } header: {
                    Text("Agent 接入")
                } footer: {
                    Text("配置写入前会自动备份；Codex Hook 信任需在官方界面确认。")
                }

                Section("Provider 数据") {
                    Toggle(isOn: Binding(
                        get: { model.claudeQuotaBridge?.status == "installed" },
                        set: { enabled in
                            let action = enabled && model.claudeQuotaBridge?.status == "custom_conflict"
                                ? "wrap"
                                : enabled ? "install" : "uninstall"
                            Task { await model.changeClaudeQuotaBridge(action: action) }
                        }
                    )) {
                        SettingsLabel("Claude 状态栏额度补充（可选）", detail: bridgeStatusText)
                    }
                    .disabled(model.isSettingsBusy || model.claudeQuotaBridge?.status == "config_malformed")

                    HStack {
                        SettingsLabel(
                            "主动更新额度",
                            detail: "每分钟自动更新，启动和唤醒后立即恢复；凭据到期自动续期，无需发送对话。立即更新会等待实际结果"
                        )
                        Spacer(minLength: 12)
                        Button(
                            localized(
                                model.isQuotaRefreshBusy ? "正在更新…" : "立即更新",
                                locale: locale
                            ),
                            systemImage: "arrow.clockwise"
                        ) {
                            Task { await model.refreshQuotaNow() }
                        }
                        .disabled(
                            !model.bridgeStatus.isListening
                                || model.isQuotaRefreshBusy
                        )
                    }
                    if let message = model.quotaRefreshMessage {
                        Text(localized(message, locale: locale))
                            .font(.caption)
                            .foregroundStyle(
                                message.contains("失败") || message.contains("未连接")
                                    ? Color.red
                                    : Color.secondary
                            )
                            .textSelection(.enabled)
                            .accessibilityLabel(localizedFormat(
                                "额度更新结果：%@",
                                locale: locale,
                                message
                            ))
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.codexEnhancedActivity },
                        set: { enabled in model.updateUISettings { $0.codexEnhancedActivity = enabled } }
                    )) {
                        SettingsLabel(
                            "Codex 增强活动",
                            detail: "关闭后仍保留审批与必要生命周期事件"
                        )
                    }
                }

                Section {
                    Picker(
                        selection: Binding(
                            get: { model.uiSettings.completionTaskHideMode },
                            set: { mode in
                                model.updateUISettings {
                                    $0.completionTaskHideMode = mode
                                }
                            }
                        ),
                        label: SettingsLabel(
                            "隐藏方式",
                            detail: "任务必须先由 Runtime 明确认定完成"
                        )
                    ) {
                        Text("确认后隐藏").tag(CompletionTaskHideMode.afterConfirmation)
                        Text("自动隐藏").tag(CompletionTaskHideMode.afterDelay)
                    }
                    .pickerStyle(.segmented)

                    if model.uiSettings.completionTaskHideMode == .afterDelay {
                        Picker(
                            selection: Binding(
                                get: { model.uiSettings.completionAutoHideMinutes },
                                set: { minutes in
                                    model.updateUISettings {
                                        $0.completionAutoHideMinutes = minutes
                                    }
                                }
                            ),
                            label: SettingsLabel(
                                "完成后保留",
                                detail: "“知道了”只关闭提醒，任务仍在设定时间自动隐藏"
                            )
                        ) {
                            Text("5 分钟").tag(UInt32(5))
                            Text("15 分钟").tag(UInt32(15))
                            Text("30 分钟").tag(UInt32(30))
                            Text("60 分钟").tag(UInt32(60))
                        }
                    }
                } header: {
                    Text("已完成任务")
                } footer: {
                    Text("正在运行、等待授权、等待回答和报错任务不会因为没有新事件而自动隐藏。隐藏不会删除会话、事件或 Token 统计。")
                }

                if ProductScope.companionManagementEnabled {
                    Section {
                        Toggle(isOn: $model.companionAllowsControl) {
                        SettingsLabel(
                            "允许处理 Agent 请求",
                            detail: "仅为这次新配对授予审批、拒绝、交回原 Agent 和问题回答能力"
                        )
                    }
                        .disabled(model.isCompanionBusy)

                    HStack {
                        SettingsLabel(
                            "Display Companion",
                            detail: "通过本机加密令牌读取任务状态；不共享 ActRealm Cookie 或数据库"
                        )
                        Spacer(minLength: 12)
                        Button("生成配对码", systemImage: "link.badge.plus") {
                            Task {
                                if await model.createDisplayCompanionPairing(),
                                   let code = model.companionPairing?.enrollmentCode
                                {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(code, forType: .string)
                                    model.showToast(AppLocalization.localized(
                                        "显示器伴生应用配对码已复制"
                                    ))
                                }
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .disabled(!model.bridgeStatus.isListening || model.isCompanionBusy)
                    }

                    if let pairing = model.companionPairing {
                        VStack(alignment: .leading, spacing: 7) {
                            HStack {
                                Text(pairing.enrollmentCode)
                                    .font(.system(.caption, design: .monospaced).weight(.semibold))
                                    .lineLimit(1)
                                    .textSelection(.enabled)
                                Spacer()
                                Button("复制") {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(
                                        pairing.enrollmentCode,
                                        forType: .string
                                    )
                                }
                            }
                            Text("5 分钟内粘贴到显示器伴生应用的 Agent 页面；配对码只能使用一次。")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }

                    ForEach(model.companionConnections) { connection in
                        HStack {
                            SettingsLabel(
                                connection.clientName,
                                detail: connection.scopes.contains("attention.respond")
                                    ? "任务状态、跳转与受控处理"
                                    : "只读任务状态与跳转"
                            )
                            Spacer(minLength: 12)
                            Button("撤销", role: .destructive) {
                                Task { await model.revokeCompanion(id: connection.id) }
                            }
                            .disabled(model.isCompanionBusy)
                        }
                    }

                    if let error = model.companionPairingError {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(.red)
                            .textSelection(.enabled)
                    }
                    } header: {
                        Text("本机伴生应用")
                    } footer: {
                        Text("撤销后对应伴生应用会立即失去访问权限；每次操作仍由 Runtime 重新校验请求和通道。")
                    }
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
    }

    private func providerRow(_ provider: SetupInfo.ProviderSetup) -> some View {
        HStack(spacing: 12) {
            ProviderAvatar(
                kind: ProviderKind(record: provider.provider) ?? .codex,
                size: 28
            )
            SettingsLabel(providerName(provider), detail: provider.statusText)
            Spacer(minLength: 12)
            providerActions(provider)
        }
    }

    @ViewBuilder
    private func providerActions(_ provider: SetupInfo.ProviderSetup) -> some View {
        if provider.canRepair == true {
            setupButton("修复", provider: provider.provider, action: "repair", prominent: true)
        } else {
            switch provider.status {
            case "not_installed":
                setupButton("安全接入", provider: provider.provider, action: "install", prominent: true)
            case "needs_reinstall":
                setupButton("重新安装", provider: provider.provider, action: "install", prominent: true)
            case "needs_trust":
                if provider.reviewCommand != nil {
                    Button("复制信任命令") { copyTrustCommand(provider) }
                }
                setupButton("移除", provider: provider.provider, action: "uninstall")
            case "installed_unverified", "connected":
                setupButton("移除", provider: provider.provider, action: "uninstall")
            case "provider_missing", "cli_missing":
                Button("安装说明…", action: openGuide)
            default:
                Button("刷新") { Task { await model.refreshSetup() } }
            }
        }
    }

    @ViewBuilder
    private func setupButton(
        _ label: String,
        provider: String,
        action: String,
        prominent: Bool = false
    ) -> some View {
        if prominent {
            Button {
                Task { await model.changeSetup(provider: provider, action: action) }
            } label: {
                Text(LocalizedStringKey(label))
            }
            .buttonStyle(.borderedProminent)
            .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
        } else {
            Button {
                Task { await model.changeSetup(provider: provider, action: action) }
            } label: {
                Text(LocalizedStringKey(label))
            }
            .buttonStyle(.bordered)
            .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
        }
    }

    private func providerName(_ provider: SetupInfo.ProviderSetup) -> String {
        provider.provider == "claude" ? "Claude Code" : "Codex"
    }

    private func copyTrustCommand(_ provider: SetupInfo.ProviderSetup) {
        guard let command = provider.reviewCommand else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(command, forType: .string)
        model.showToast(localized("Codex 启动命令已复制；运行后输入 /hooks", locale: locale))
    }

    private func openGuide() {
        guard let url = URL(string: "https://github.com/Frontier-Interfaces/ActRealm/blob/agent/v1-full/docs/USER_GUIDE_zh-CN.md") else { return }
        NSWorkspace.shared.open(url)
    }

    private var bridgeStatusText: String {
        switch model.claudeQuotaBridge?.status {
        case "installed": "已开启；支持自动同步和主动更新"
        case "not_installed": "未开启"
        case "helper_missing": "相关文件缺失，可以安全修复"
        case "custom_conflict": "检测到自定义状态栏；开启时会保留原显示"
        case "config_malformed": "Claude 配置无法解析，已停止修改"
        default: "状态暂时不可用"
        }
    }

}

private struct NotificationSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @State private var displayCatalogRevision = 0

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "通知",
                subtitle: "管理本机 Agent 的待处理事项与通知。"
            )
            Form {
                Section {
                    notificationRow(
                        title: "等待批准",
                        detail: "Agent 请求执行操作，需要你的批准",
                        symbol: "checkmark.shield",
                        kind: "approval"
                    )
                    notificationRow(
                        title: "等待回答",
                        detail: "Agent 提出了需要你回答的问题",
                        symbol: "questionmark.bubble",
                        kind: "question"
                    )
                    notificationRow(
                        title: "需要处理",
                        detail: "Agent 执行出错，或长时间没有进展",
                        symbol: "exclamationmark.triangle",
                        kind: "error"
                    )
                    notificationRow(
                        title: "等待确认",
                        detail: "Agent 已完成本轮任务，需要你的确认",
                        symbol: "checkmark.circle",
                        kind: "completion"
                    )
                } header: {
                    Text("进入 Outbox 的事件")
                } footer: {
                    Text("关闭某项后，此类事件仍会保留在任务记录中，但不会出现在 Outbox。")
                }


                Section("声音") {
                    Toggle(isOn: Binding(
                        get: { model.uiSettings.soundEnabled },
                        set: { enabled in model.updateUISettings { $0.soundEnabled = enabled } }
                    )) {
                        SettingsLabel("提示音", detail: "新事件进入 Outbox 时播放本机轻提示音")
                    }
                }

                Section {
                    Toggle(isOn: Binding(
                        get: { model.hudSettings.isEnabled },
                        set: { enabled in model.updateHUDSettings { $0.isEnabled = enabled } }
                    )) {
                        SettingsLabel("显示 HUD 胶囊", detail: "新事件到达时在目标显示器的安全区域顶部居中")
                    }

                    Picker(selection: hudDisplayModeBinding) {
                        Text("系统主显示器").tag(HUDDisplayMode.systemMain)
                        Text("指定显示器").tag(HUDDisplayMode.selectedDisplay)
                        Text("跟随 ActRealm 窗口").tag(HUDDisplayMode.followActRealmWindow)
                    } label: {
                        SettingsLabel("显示位置", detail: hudDisplayModeDetail)
                    }
                    .disabled(!model.hudSettings.isEnabled)

                    if model.hudSettings.displayMode == .selectedDisplay {
                        Picker(selection: selectedDisplayBinding) {
                            ForEach(displayOptions) { display in
                                Text(display.isMain
                                    ? localizedFormat(
                                        "%@（主显示器）",
                                        locale: locale,
                                        display.name
                                    )
                                    : display.name)
                                    .tag(display.id)
                            }
                            if let selectedID = model.hudSettings.selectedDisplayID,
                               resolvedSelectedDisplay == nil
                            {
                                Text(localizedFormat(
                                    "%@（未连接）",
                                    locale: locale,
                                    model.hudSettings.selectedDisplayName
                                        ?? localized("显示器", locale: locale)
                                ))
                                    .tag(selectedID)
                            }
                        } label: {
                            SettingsLabel("目标显示器", detail: selectedDisplayDetail)
                        }
                        .disabled(!model.hudSettings.isEnabled || displayOptions.isEmpty)
                    }

                    Picker(selection: Binding(
                        get: { model.hudSettings.displaySeconds },
                        set: { seconds in model.updateHUDSettings { $0.displaySeconds = seconds } }
                    )) {
                        Text("5 秒").tag(5)
                        Text("8 秒").tag(8)
                        Text("12 秒").tag(12)
                        Text("20 秒").tag(20)
                    } label: {
                        SettingsLabel("显示时间", detail: "智能聚焦 HUD 的等待时间由 Agent Focus 单独设置")
                    }
                    .disabled(!model.hudSettings.isEnabled)

                    VStack(alignment: .leading, spacing: 9) {
                        Text("显示字段")
                        LazyVGrid(
                            columns: [GridItem(.flexible()), GridItem(.flexible())],
                            alignment: .leading,
                            spacing: 8
                        ) {
                            hudField("Agent", field: .provider)
                            hudField("事件类型", field: .event)
                            hudField("任务摘要", field: .task)
                            hudField("项目", field: .project)
                            hudField("等待时间", field: .elapsed)
                        }
                    }
                    .disabled(!model.hudSettings.isEnabled)

                    HStack {
                        Spacer()
                        Button("测试胶囊") { model.previewHUD() }
                            .disabled(!model.hudSettings.isEnabled)
                    }
                } header: {
                    Text("HUD 胶囊")
                } footer: {
                    Text("指定显示器断开时会暂时回退到系统主显示器；审批按钮始终保留。")
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
        .onReceive(NotificationCenter.default.publisher(
            for: NSApplication.didChangeScreenParametersNotification
        )) { _ in
            displayCatalogRevision &+= 1
        }
    }

    private var displayOptions: [HUDDisplayOption] {
        _ = displayCatalogRevision
        return HUDDisplayCatalog.options
    }

    private var hudDisplayModeBinding: Binding<HUDDisplayMode> {
        Binding(
            get: { model.hudSettings.displayMode },
            set: { mode in
                let displays = displayOptions
                model.updateHUDSettings { settings in
                    settings.displayMode = mode
                    if mode == .selectedDisplay,
                       settings.selectedDisplayID == nil,
                       settings.selectedDisplayName == nil,
                       let display = displays.first
                    {
                        settings.selectedDisplayID = display.id
                        settings.selectedDisplayName = display.name
                    }
                }
            }
        )
    }

    private var selectedDisplayBinding: Binding<UInt32> {
        let displays = displayOptions
        let fallbackID = displays.first?.id ?? CGMainDisplayID()
        return Binding(
            get: {
                resolvedSelectedDisplay?.id
                    ?? model.hudSettings.selectedDisplayID
                    ?? fallbackID
            },
            set: { displayID in
                guard let display = displays.first(where: { $0.id == displayID }) else { return }
                model.updateHUDSettings { settings in
                    settings.selectedDisplayID = display.id
                    settings.selectedDisplayName = display.name
                }
            }
        )
    }

    private var hudDisplayModeDetail: String {
        let key = switch model.hudSettings.displayMode {
        case .systemMain:
            "始终显示在 macOS 当前的主显示器"
        case .selectedDisplay:
            "固定显示在下方选择的显示器"
        case .followActRealmWindow:
            "ActRealm 主窗口跨屏移动后，HUD 会同步跟随"
        }
        return localized(key, locale: locale)
    }

    private var selectedDisplayDetail: String {
        guard resolvedSelectedDisplay != nil else {
            return localized("所选显示器未连接时暂用系统主显示器", locale: locale)
        }
        return localized("当前已连接", locale: locale)
    }

    private var resolvedSelectedDisplay: HUDDisplayOption? {
        if let selectedID = model.hudSettings.selectedDisplayID,
           let display = displayOptions.first(where: { $0.id == selectedID })
        {
            return display
        }
        guard let selectedName = model.hudSettings.selectedDisplayName else { return nil }
        return displayOptions.first { $0.name == selectedName }
    }

    private func notificationRow(
        title: String,
        detail: String,
        symbol: String,
        kind: String
    ) -> some View {
        Toggle(isOn: notificationBinding(kind)) {
            Label {
                SettingsLabel(title, detail: detail)
            } icon: {
                Image(systemName: symbol)
                    .foregroundStyle(.secondary)
            }
        }
        .toggleStyle(.switch)
    }

    private func notificationBinding(_ kind: String) -> Binding<Bool> {
        Binding(
            get: { model.uiSettings.notificationRules.mode(for: kind) != .ignore },
            set: { isEnabled in
                model.updateUISettings { settings in
                    let mode: NotificationMode = isEnabled ? .list : .ignore
                    switch kind {
                    case "approval": settings.notificationRules.approval = mode
                    case "question": settings.notificationRules.question = mode
                    case "error": settings.notificationRules.error = mode
                    case "completion": settings.notificationRules.completion = mode
                    default: break
                    }
                }
            }
        )
    }


    private func hudField(_ label: String, field: HUDDisplayField) -> some View {
        Toggle(isOn: Binding(
            get: { model.hudSettings.fields.contains(field) },
            set: { enabled in
                model.updateHUDSettings { settings in
                    if enabled, !settings.fields.contains(field) {
                        settings.fields.append(field)
                    } else if !enabled {
                        settings.fields.removeAll { $0 == field }
                    }
                }
            }
        )) {
            Text(LocalizedStringKey(label))
        }
        .toggleStyle(.checkbox)
    }

}

private struct ThemeSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @State private var importError: String?

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "主题",
                subtitle: "调整工作区背景、三栏透明度与窗口失焦外观。"
            )
            Form {
                Section {
                    ThemeLanePreview(
                        backgroundURL: model.themeBackgroundURL,
                        backgroundKind: model.themeSettings.backgroundKind,
                        laneOpacity: model.themeSettings.laneOpacity,
                        maintainsTransparencyWhenInactive:
                            model.themeSettings.maintainsTransparencyWhenInactive
                    )
                    .frame(maxWidth: .infinity)
                    .aspectRatio(model.mainWindowAspectRatio, contentMode: .fit)
                    .frame(maxHeight: 320)
                    .clipped()
                    .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .strokeBorder(Color.white.opacity(0.28), lineWidth: 1)
                    )

                    HStack {
                        SettingsLabel(
                            "工作区背景",
                            detail: hasCustomBackground
                                ? localizedFormat(
                                    "%@已复制到 ActRealm 的本地应用数据目录",
                                    locale: locale,
                                    backgroundKindLabel
                                )
                                : "正在使用默认玻璃背景"
                        )
                        Spacer()
                        if hasCustomBackground {
                            Button("恢复默认", role: .destructive) {
                                model.resetThemeBackground()
                            }
                        }
                        Button(ProductScope.animatedThemeMediaEnabled
                            ? "选择图片 / GIF / 视频…"
                            : "选择图片…", action: chooseBackground)
                            .buttonStyle(.borderedProminent)
                    }
                } header: {
                    Text("背景图片")
                } footer: {
                    Text(ProductScope.animatedThemeMediaEnabled
                        ? "支持静态图片、GIF、MP4、MOV 等 macOS 可读取格式。GIF 与视频会静音自动循环；文件只保存在本机。"
                        : "只使用本机静态图片；动画与视频背景在当前候选中暂停。")
                }

                Section {
                    LabeledContent {
                        Text("\(Int((model.themeSettings.laneOpacity * 100).rounded()))%")
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    } label: {
                        SettingsLabel("三栏不透明度", detail: "同时调整 OUTBOX、AGENT TASKS 与 QUOTA")
                    }

                    Slider(
                        value: Binding(
                            get: { model.themeSettings.laneOpacity },
                            set: { value in
                                model.updateThemeSettings { $0.laneOpacity = value }
                            }
                        ),
                        in: 0...1
                    ) {
                        Text("三栏不透明度")
                    } minimumValueLabel: {
                        Text("0% 透明")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } maximumValueLabel: {
                        Text("100% 不透明")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                } header: {
                    Text("三栏外观")
                } footer: {
                    Text("0% 为完全透明，100% 为完全不透明。上方预览会按主窗口当前比例实时显示最终叠加效果。")
                }

                Section {
                    Toggle(isOn: Binding(
                        get: { model.themeSettings.maintainsTransparencyWhenInactive },
                        set: { enabled in
                            model.updateThemeSettings {
                                $0.maintainsTransparencyWhenInactive = enabled
                            }
                        }
                    )) {
                        SettingsLabel(
                            "失焦时保持透明度",
                            detail: "ActRealm 不在前台时，背景与三栏仍保持当前透明度"
                        )
                    }
                } header: {
                    Text("窗口失焦")
                } footer: {
                    Text("默认开启。关闭后恢复 macOS 原生玻璃行为，窗口失焦时材质会自动变厚。")
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
        .alert(
            "无法使用这张图片",
            isPresented: Binding(
                get: { importError != nil },
                set: { if !$0 { importError = nil } }
            )
        ) {
            Button("好", role: .cancel) { importError = nil }
        } message: {
            Text(localized(importError ?? "请选择另一张图片。", locale: locale))
        }
    }

    private var hasCustomBackground: Bool { model.themeBackgroundURL != nil }

    private var backgroundKindLabel: String {
        let key = switch model.themeSettings.backgroundKind {
        case .image: "静态图片"
        case .animatedImage: "GIF"
        case .video: "循环视频"
        }
        return localized(key, locale: locale)
    }

    private func chooseBackground() {
        let panel = NSOpenPanel()
        panel.title = localized("选择 ActRealm 背景", locale: locale)
        panel.prompt = localized("使用背景", locale: locale)
        panel.allowedContentTypes = ProductScope.animatedThemeMediaEnabled
            ? [.image, .movie]
            : [.image]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task {
            await importBackground(from: url)
        }
    }

    @MainActor
    private func importBackground(from url: URL) async {
        let contentType = try? url.resourceValues(forKeys: [.contentTypeKey]).contentType
        let kind: ThemeBackgroundKind
        if contentType?.conforms(to: .movie) == true {
            kind = .video
        } else if url.pathExtension.lowercased() == "gif" {
            kind = .animatedImage
        } else {
            kind = .image
        }

        let hasScopedAccess = url.startAccessingSecurityScopedResource()
        defer {
            if hasScopedAccess { url.stopAccessingSecurityScopedResource() }
        }

        do {
            switch kind {
            case .video:
                let asset = AVURLAsset(url: url)
                guard try await asset.load(.isPlayable) else {
                    importError = "视频无法播放，请选择 MP4、MOV 或其他 macOS 支持的视频。"
                    return
                }
            case .image, .animatedImage:
                guard NSImage(contentsOf: url) != nil else {
                    importError = "图片无法解码，请选择 PNG、JPEG、HEIC 或 GIF。"
                    return
                }
            }
            try model.importThemeBackground(from: url, kind: kind)
        } catch {
            importError = error.localizedDescription
        }
    }
}

private struct ThemeLanePreview: View {
    @Environment(\.locale) private var locale
    let backgroundURL: URL?
    let backgroundKind: ThemeBackgroundKind
    let laneOpacity: Double
    let maintainsTransparencyWhenInactive: Bool

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                background

                HStack(spacing: 8) {
                    previewLane(title: "OUTBOX", rows: 2)
                        .frame(width: max(80, (proxy.size.width - 16) * 0.28))
                    previewLane(title: "AGENT TASKS", rows: 3)
                        .frame(maxWidth: .infinity)
                    previewLane(title: "QUOTA", rows: 2)
                        .frame(width: max(80, (proxy.size.width - 16) * 0.24))
                }
                .padding(14)
            }
        }
        .background(Color(nsColor: .controlBackgroundColor))
        .overlay(alignment: .topTrailing) {
            Text(backgroundLabel)
                .font(.system(size: 9, weight: .bold))
                .foregroundStyle(.white)
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(Color.black.opacity(0.42), in: Capsule())
                .padding(9)
        }
    }

    @ViewBuilder
    private var background: some View {
        if let backgroundURL {
            AppThemeBackdrop(
                url: backgroundURL,
                kind: backgroundKind,
                maintainsTransparencyWhenInactive: maintainsTransparencyWhenInactive
            )
        } else {
            LinearGradient(
                colors: [
                    DT.logoTint.opacity(0.55),
                    Color(red: 0.96, green: 0.9, blue: 0.72),
                    DT.greenDot.opacity(0.28),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        }
    }

    private func previewLane(title: String, rows: Int) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(title)
                .font(.system(size: 9.5, weight: .heavy))
                .foregroundStyle(DT.textPrimary)
            ForEach(0..<rows, id: \.self) { row in
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(DT.cardStrong.opacity(0.72))
                    .frame(height: row == 0 ? 34 : 22)
                    .overlay(alignment: .leading) {
                        Capsule()
                            .fill(row == 0 ? DT.logoTint.opacity(0.5) : DT.textFaint.opacity(0.18))
                            .frame(width: row == 0 ? 42 : 58, height: 4)
                            .padding(.leading, 8)
                    }
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .frame(maxHeight: .infinity, alignment: .topLeading)
        .modifier(ThemedLaneSurface(
            opacity: laneOpacity,
            maintainsTransparencyWhenInactive: maintainsTransparencyWhenInactive,
            radius: 13,
            stroke: Color.white.opacity(0.55),
            shadow: .clear,
            shadowRadius: 0,
            shadowY: 0
        ))
    }

    private var backgroundLabel: String {
        guard backgroundURL != nil else { return localized("默认背景", locale: locale) }
        let key = switch backgroundKind {
        case .image: "静态图片"
        case .animatedImage: "GIF 循环"
        case .video: "视频循环 · 静音"
        }
        return localized(key, locale: locale)
    }
}

private struct DisplayFieldPlacement: Identifiable {
    let id: String
    let title: String
    let detail: String
}

private struct TaskCardFieldGuide: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("任务卡位置示意")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(DT.textSecondary)
            HStack(spacing: 8) {
                Text("主标题")
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(DT.textStrong)
                Text("状态")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(DT.amberText)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(DT.amberBg, in: Capsule())
                Spacer(minLength: 4)
                Text("实时状态")
                    .font(.system(size: 9.5, weight: .semibold))
                    .foregroundStyle(DT.textWeak)
            }
            Text("任务摘要 · 主标题不同时显示在第二行")
                .font(.system(size: 10))
                .foregroundStyle(DT.textSecondary)
            Text("副标题 · 项目 · 模型 · 计划进度")
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textWeak)
            HStack(spacing: 6) {
                guideChip("Token")
                guideChip("上下文")
                guideChip("API 等价值")
            }
            Divider()
            Label("点击任务后展开详细信息与开发者信息", systemImage: "chevron.down")
                .font(.system(size: 9.5))
                .foregroundStyle(DT.textFaint)
        }
        .padding(11)
        .background(DT.neutralChipBg.opacity(0.75), in: RoundedRectangle(cornerRadius: 11, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 11, style: .continuous)
                .strokeBorder(DT.neutralChipStroke, lineWidth: 1)
        )
    }

    private func guideChip(_ text: String) -> some View {
        Text(LocalizedStringKey(text))
            .font(.system(size: 8.5, weight: .medium))
            .foregroundStyle(DT.textWeak)
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(DT.cardStrong.opacity(0.7), in: Capsule())
            .overlay(Capsule().strokeBorder(DT.neutralChipStroke, lineWidth: 1))
    }
}

private struct DisplaySettingsPage: View {
    @EnvironmentObject private var model: AppModel

    private let presets = TaskCardDisplayPresets.all

    private let placements = [
        DisplayFieldPlacement(id: "headline", title: "主标题与状态", detail: "折叠任务卡的第一、二行"),
        DisplayFieldPlacement(id: "subtitle", title: "副标题与进度", detail: "折叠任务卡的身份信息与计划"),
        DisplayFieldPlacement(id: "overview", title: "用量概览", detail: "折叠任务卡中的用量胶囊"),
        DisplayFieldPlacement(id: "details", title: "展开详情", detail: "点击任务卡后显示"),
        DisplayFieldPlacement(id: "developer", title: "开发者信息", detail: "展开详情中的来源与内部标识"),
    ]

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "显示",
                subtitle: "控制任务卡、Token 用量和额度卡的信息密度；只显示 Runtime 允许的安全字段。"
            )
            Form {
                Section {
                    Picker(
                        selection: Binding(
                            get: { model.uiSettings.quotaDisplayMode },
                            set: { mode in
                                model.updateUISettings { $0.quotaDisplayMode = mode }
                            }
                        ),
                        label: SettingsLabel(
                            "额度卡片",
                            detail: "完整保留全部信息；紧凑使用双行；单行把核心额度排在一行"
                        )
                    ) {
                        Text("完整").tag(QuotaDisplayMode.full)
                        Text("紧凑").tag(QuotaDisplayMode.compact)
                        Text("单行").tag(QuotaDisplayMode.singleLine)
                    }
                    .pickerStyle(.segmented)

                } header: {
                    Text("额度显示")
                } footer: {
                    Text("默认使用完整模式。三种模式都会随额度栏宽度自适应，且不改变额度数据与刷新规则。")
                }

                    Section {
                    Picker(
                        selection: Binding(
                            get: { model.uiSettings.tokenUsageDisplayMode },
                            set: { mode in
                                model.updateUISettings { $0.tokenUsageDisplayMode = mode }
                            }
                        ),
                        label: SettingsLabel(
                            "累计 Token",
                            detail: "完整显示入口摘要；紧凑只保留核心数字；点击可打开独立 Token 仪表板"
                        )
                    ) {
                        Text("完整").tag(TokenUsageDisplayMode.full)
                        Text("紧凑").tag(TokenUsageDisplayMode.compact)
                        Text("隐藏").tag(TokenUsageDisplayMode.hidden)
                    }
                    .pickerStyle(.segmented)

                    if ProductScope.advancedTokenAnalyticsEnabled {
                        Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageComponentsVisible },
                        set: { visible in
                            model.updateUISettings {
                                $0.tokenUsageComponentsVisible = visible
                            }
                        }
                    )) {
                        SettingsLabel(
                            "Token 组成",
                            detail: "在独立仪表板显示未命中输入、缓存读取、缓存写入、输出与命中率"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageHeatmapVisible },
                        set: { visible in
                            model.updateUISettings { $0.tokenUsageHeatmapVisible = visible }
                        }
                    )) {
                        SettingsLabel(
                            "Token 活跃度",
                            detail: "显示逐日 Token / API 等价值热力图与悬停详情"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageCostVisible },
                        set: { visible in
                            model.updateUISettings { $0.tokenUsageCostVisible = visible }
                        }
                    )) {
                        SettingsLabel(
                            "估算 API 费用",
                            detail: "显示价格快照能够覆盖的 API 等价估算；未知不会显示为 0"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageExecutionTimeVisible },
                        set: { visible in
                            model.updateUISettings {
                                $0.tokenUsageExecutionTimeVisible = visible
                            }
                        }
                    )) {
                        SettingsLabel(
                            "Agent 执行时间",
                            detail: "仅统计思考、工具运行与上下文压缩区间；排除等待，并发任务相加"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageObservedTimeVisible },
                        set: { visible in
                            model.updateUISettings {
                                $0.tokenUsageObservedTimeVisible = visible
                            }
                        }
                    )) {
                        SettingsLabel(
                            "任务观测时间",
                            detail: "按日、月、累计显示 Turn 开始到最后事件的区间（包含等待）"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageTaskProjectVisible },
                        set: { visible in
                            model.updateUISettings {
                                $0.tokenUsageTaskProjectVisible = visible
                            }
                        }
                    )) {
                        SettingsLabel(
                            "任务与项目归因",
                            detail: "项目来自脱敏会话元数据；任务使用可验证会话与父子关系，分别保留未识别数量"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageBurnRateVisible },
                        set: { visible in
                            model.updateUISettings {
                                $0.tokenUsageBurnRateVisible = visible
                            }
                        }
                    )) {
                        SettingsLabel(
                            "当前燃烧速度",
                            detail: "使用真实 5 分钟滑动窗口；只比较数值，不判断是否浪费"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenUsageAnomalyVisible },
                        set: { visible in
                            model.updateUISettings {
                                $0.tokenUsageAnomalyVisible = visible
                            }
                        }
                    )) {
                        SettingsLabel(
                            "用量异常与数据质量",
                            detail: "显示数值突增、账本一致性和价格覆盖问题"
                        )
                    }

                    Toggle(isOn: Binding(
                        get: { model.uiSettings.tokenThresholdNotificationsEnabled },
                        set: { enabled in
                            model.updateUISettings {
                                $0.tokenThresholdNotificationsEnabled = enabled
                            }
                        }
                    )) {
                        SettingsLabel(
                            "燃烧速度提醒",
                            detail: "仅对实时任务触发；历史回补和首次基线不会通知"
                        )
                    }

                    if model.uiSettings.tokenThresholdNotificationsEnabled {
                        Picker(
                            selection: Binding(
                                get: { model.uiSettings.tokenThresholdTokensPerMinute },
                                set: { threshold in
                                    model.updateUISettings {
                                        $0.tokenThresholdTokensPerMinute = threshold
                                    }
                                }
                            ),
                            label: SettingsLabel(
                                "提醒阈值",
                                detail: "本机 Token / 分钟；不与官方套餐额度换算"
                            )
                        ) {
                            Text("50K / 分钟").tag(UInt64(50_000))
                            Text("100K / 分钟").tag(UInt64(100_000))
                            Text("250K / 分钟").tag(UInt64(250_000))
                            Text("500K / 分钟").tag(UInt64(500_000))
                            Text("1M / 分钟").tag(UInt64(1_000_000))
                        }
                    }

                    Picker(
                        selection: Binding(
                            get: { model.uiSettings.tokenUsageUnitStyle },
                            set: { style in
                                model.updateUISettings { $0.tokenUsageUnitStyle = style }
                            }
                        ),
                        label: SettingsLabel(
                            "Token 简写",
                            detail: "自动模式会在中文界面使用万/亿，英文界面使用 K/M/B"
                        )
                    ) {
                        Text("自动").tag(TokenUsageUnitStyle.automatic)
                        Text("K / M / B").tag(TokenUsageUnitStyle.western)
                        Text("万 / 亿").tag(TokenUsageUnitStyle.eastAsian)
                    }
                        .pickerStyle(.segmented)
                    }
                    } header: {
                        Text("Token 用量")
                    } footer: {
                        Text("Codex 与 Claude 使用同一套本机统计。只有 Agent 提供真实 Token 数据后才显示数值；不会用额度百分比推算。")
                    }
                Section("显示档位") {
                    Picker("任务卡", selection: Binding(
                        get: { activePreset },
                        set: { profile in applyPreset(profile) }
                    )) {
                        Text("简洁").tag("concise")
                        Text("详细").tag("detailed")
                        Text("开发者").tag("developer")
                    }
                    .pickerStyle(.segmented)
                }

                if ProductScope.developerDisplayCustomizationEnabled {
                    Section {
                    Toggle(isOn: Binding(
                        get: { model.uiSettings.displayProfile == "custom" },
                        set: { enabled in setCustom(enabled) }
                    )) {
                        SettingsLabel("自定义字段", detail: "开启后可逐项编辑下方任务卡字段")
                    }

                    if model.displayCatalog.isEmpty {
                        ProgressView("正在读取可用字段…")
                            .controlSize(.small)
                    } else {
                        TaskCardFieldGuide()
                            .padding(.bottom, 4)

                        ForEach(placements) { placement in
                            let fields = fields(in: placement.id)
                            if !fields.isEmpty {
                                VStack(alignment: .leading, spacing: 8) {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(LocalizedStringKey(placement.title))
                                            .font(.system(size: 11, weight: .semibold))
                                            .foregroundStyle(DT.textSecondary)
                                        Text(LocalizedStringKey(placement.detail))
                                            .font(.system(size: 9.5))
                                            .foregroundStyle(DT.textFaint)
                                    }

                                    LazyVGrid(
                                        columns: [GridItem(.flexible()), GridItem(.flexible())],
                                        alignment: .leading,
                                        spacing: 10
                                    ) {
                                        ForEach(fields) { field in
                                            Toggle(isOn: fieldBinding(field)) {
                                                VStack(alignment: .leading, spacing: 2) {
                                                    Text(AppLocalization.localizedDisplayFieldLabel(
                                                        id: field.id,
                                                        fallback: field.label,
                                                        language: model.appLanguage
                                                    ))
                                                        .font(.system(size: 11))
                                                    if let description = AppLocalization
                                                        .localizedDisplayFieldDescription(
                                                            id: field.id,
                                                            fallback: field.description,
                                                            language: model.appLanguage
                                                        ),
                                                       !description.isEmpty {
                                                        Text(description)
                                                            .font(.system(size: 9))
                                                            .foregroundStyle(DT.textFaint)
                                                            .fixedSize(horizontal: false, vertical: true)
                                                    }
                                                }
                                            }
                                            .toggleStyle(.checkbox)
                                            .disabled(model.uiSettings.displayProfile != "custom")
                                        }
                                    }
                                }
                                .padding(.vertical, 5)
                            }
                        }
                    }
                    } header: {
                        Text("任务卡字段")
                    } footer: {
                        Text("三个预设提供固定字段组合；简洁模式默认显示 7 项折叠信息，并保留展开后的任务流程与工作流。开启“自定义字段”后可按显示位置逐项调整。原始提示、命令和文件内容不会因此显示。")
                    }
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
    }

    private func applyPreset(_ profile: String) {
        model.updateUISettings {
            $0.displayProfile = profile
            $0.taskCardFields = presets[profile] ?? presets["detailed"]!
        }
    }

    private var activePreset: String {
        if presets[model.uiSettings.displayProfile] != nil {
            return model.uiSettings.displayProfile
        }
        return presets.first(where: { $0.value == model.uiSettings.taskCardFields })?.key ?? "detailed"
    }

    private func setCustom(_ enabled: Bool) {
        if enabled {
            model.updateUISettings { $0.displayProfile = "custom" }
        } else {
            applyPreset(activePreset)
        }
    }

    private func fieldBinding(_ field: DisplayField) -> Binding<Bool> {
        Binding(
            get: { model.uiSettings.taskCardFields.contains(field.id) },
            set: { enabled in
                model.updateUISettings { settings in
                    if enabled, !settings.taskCardFields.contains(field.id) {
                        settings.taskCardFields.append(field.id)
                    } else if !enabled {
                        settings.taskCardFields.removeAll { $0 == field.id }
                    }
                }
            }
        )
    }

    private func fields(in placement: String) -> [DisplayField] {
        model.displayCatalog.filter { field in
            let resolvedPlacement = field.placement
                ?? (field.level == "developer" ? "developer" : "details")
            return resolvedPlacement == placement
        }
    }
}

private enum DataExportKind {
    case allData
    case metrics
    case tokenJSON
    case tokenCSV
}

private struct DataSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @State private var showingClearConfirmation = false
    @State private var clearConfirmation = ""
    @State private var showingBackupClearConfirmation = false
    @State private var backupClearConfirmation = ""
    @State private var exporting = false

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "数据",
                subtitle: "管理本机数据、导出与保留期限。"
            )
            Form {
                Section("本地数据") {
                    Picker(selection: Binding(
                        get: { model.uiSettings.retentionDays },
                        set: { days in model.updateUISettings { $0.retentionDays = days } }
                    )) {
                        Text("30 天").tag(UInt32(30))
                        Text("90 天").tag(UInt32(90))
                        Text("180 天").tag(UInt32(180))
                        Text("永久").tag(UInt32(0))
                    } label: {
                        SettingsLabel("事件保留", detail: "超过保留期的本机事件会自动清理")
                    }

                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Button("导出全部数据…") { Task { await export(.allData) } }
                            Button("导出使用统计…") { Task { await export(.metrics) } }
                            Spacer()
                        }
                        HStack {
                            Button("导出 Token JSON…") { Task { await export(.tokenJSON) } }
                            Button("导出 Token CSV…") { Task { await export(.tokenCSV) } }
                            Spacer()
                        }
                        Text("Token 数值导出不包含任务 ID、Prompt、路径、命令、工具内容或回复")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .disabled(exporting || model.isDemo)
                }


                if ProductScope.localUsageStatsEnabled {
                    Section {
                        metricsGrid
                    } header: {
                        Text("使用统计")
                    } footer: {
                        Text("统计只在这台 Mac 上累计。")
                    }
                }

                Section {
                    LabeledContent {
                        Text(backupSummaryText)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    } label: {
                        SettingsLabel(
                            "配置备份",
                            detail: "ActRealm 修改 Agent 配置前创建；不会自动删除"
                        )
                    }

                    if showingBackupClearConfirmation {
                        SettingsLabel(
                            "确认清除配置备份",
                            detail: "输入 DELETE BACKUPS；不会删除当前 Agent 配置"
                        )
                        TextField("DELETE BACKUPS", text: $backupClearConfirmation)
                        HStack {
                            Button("取消") {
                                showingBackupClearConfirmation = false
                                backupClearConfirmation = ""
                            }
                            Spacer()
                            Button("清除配置备份", role: .destructive) {
                                Task {
                                    if await model.clearConfigurationBackups(
                                        confirmation: backupClearConfirmation
                                    ) {
                                        showingBackupClearConfirmation = false
                                        backupClearConfirmation = ""
                                    }
                                }
                            }
                            .disabled(backupClearConfirmation != "DELETE BACKUPS")
                        }
                    } else {
                        HStack {
                            Text("只删除 ActRealm 所有的私有备份；发现符号链接或陌生文件会拒绝操作。")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Spacer()
                            Button("清除备份…", role: .destructive) {
                                showingBackupClearConfirmation = true
                            }
                            .disabled(model.isDemo || model.backupSummary.count == 0)
                        }
                    }
                } header: {
                    Text("配置备份")
                } footer: {
                    Text("备份用于配置恢复。只有你明确确认后才会删除。")
                }

                Section {
                    if showingClearConfirmation {
                        SettingsLabel("确认彻底清除", detail: "输入 DELETE；Agent 接入和备份不会被删除")
                        TextField("DELETE", text: $clearConfirmation)
                        HStack {
                            Button("取消") {
                                showingClearConfirmation = false
                                clearConfirmation = ""
                            }
                            Spacer()
                            Button("确认清除", role: .destructive) {
                                Task {
                                    let cleared = await model.clearLocalData(confirmation: clearConfirmation)
                                    if cleared {
                                        showingClearConfirmation = false
                                        clearConfirmation = ""
                                    }
                                }
                            }
                            .disabled(clearConfirmation != "DELETE")
                        }
                    } else {
                        HStack {
                            SettingsLabel("彻底清除运行数据", detail: "不会删除 Hook 接入和配置备份")
                            Spacer()
                            Button("彻底清除…", role: .destructive) {
                                showingClearConfirmation = true
                            }
                            .disabled(model.isDemo)
                        }
                    }
                } header: {
                    Text("清除数据")
                } footer: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("此操作不可撤销。")
                    }
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
    }

    private var backupSummaryText: String {
        let byteCount = Int64(clamping: model.backupSummary.totalBytes)
        let size = ByteCountFormatter.string(fromByteCount: byteCount, countStyle: .file)
        return localizedFormat(
            "%llu 个 · %@",
            locale: locale,
            model.backupSummary.count,
            size
        )
    }

    private var metricsGrid: some View {
        let metrics = model.client.snapshot.stats.metrics
        let requests = metrics.approvalRequests
        let decisions = metrics.widgetApprovals + metrics.widgetDenials
        let panelRate = requests > 0
            ? "\(Int((Double(decisions) / Double(requests) * 100).rounded()))%"
            : "—"
        let timeoutRate = requests > 0
            ? "\(Int((Double(metrics.passThroughTimeout) / Double(requests) * 100).rounded()))%"
            : "—"
        let average = metrics.decisionResponseCount > 0
            ? String(
                format: "%.1fs",
                Double(metrics.decisionResponseMsTotal) / Double(metrics.decisionResponseCount) / 1000
            )
            : "—"

        return Grid(horizontalSpacing: 28, verticalSpacing: 14) {
            GridRow {
                metric("\(metrics.activeDays)", "活跃天数")
                metric("\(decisions)", "面板批准 / 拒绝")
                metric(panelRate, "面板处理率")
            }
            GridRow {
                metric(timeoutRate, "超时交还率")
                metric(average, "平均响应")
                metric(model.nativePresentationP95Ms.map { "\($0)ms" } ?? "—", "原生呈现 p95")
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 6)
    }

    private func metric(_ value: String, _ label: String) -> some View {
        VStack(spacing: 3) {
            Text(value)
                .font(.title3.weight(.semibold))
            Text(LocalizedStringKey(label))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
    }

    @MainActor
    private func export(_ kind: DataExportKind) async {
        guard !exporting else { return }
        exporting = true
        defer { exporting = false }
        let data: Data?
        switch kind {
        case .allData:
            data = await model.exportLocalData(metricsOnly: false)
        case .metrics:
            data = await model.exportLocalData(metricsOnly: true)
        case .tokenJSON:
            data = await model.exportTokenUsage(csv: false)
        case .tokenCSV:
            data = await model.exportTokenUsage(csv: true)
        }
        guard let data else { return }
        let panel = NSSavePanel()
        switch kind {
        case .allData:
            panel.nameFieldStringValue = "actrealm-export.json"
            panel.allowedContentTypes = [.json]
        case .metrics:
            panel.nameFieldStringValue = "actrealm-metrics.json"
            panel.allowedContentTypes = [.json]
        case .tokenJSON:
            panel.nameFieldStringValue = "actrealm-token-usage.json"
            panel.allowedContentTypes = [.json]
        case .tokenCSV:
            panel.nameFieldStringValue = "actrealm-token-usage.csv"
            panel.allowedContentTypes = [.commaSeparatedText]
        }
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try data.write(to: url, options: .atomic)
            let message = switch kind {
            case .allData: "本地数据已导出"
            case .metrics: "统计已导出"
            case .tokenJSON, .tokenCSV: "Token 数值已导出"
            }
            model.showToast(localized(message, locale: locale))
        } catch {
            model.showToast(
                localizedFormat("保存失败：%@", locale: locale, error.localizedDescription),
                priority: .error
            )
        }
    }
}
