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

    var title: String {
        switch self {
        case .general: "通用"
        case .agents: "Agent"
        case .notifications: "通知"
        case .theme: "主题"
        case .display: "显示"
        case .data: "数据"
        }
    }

}

public struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    @State private var selection: SettingsSection

    public init(initialSection: SettingsSection = .general) {
        _selection = State(initialValue: initialSection)
    }

    public var body: some View {
        HStack(spacing: 0) {
            List(SettingsSection.allCases, selection: $selection) { section in
                Text(section.title)
                    .tag(section)
            }
            .listStyle(.sidebar)
            .frame(width: 168)

            Divider()

            detail
                .id(selection)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: 920, height: 660)
        .background(Color(nsColor: .windowBackgroundColor))
        .task {
            model.refreshRuntimeDiagnostics()
            await model.refreshSettings()
            await model.refreshSetup()
        }
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

private struct SettingsPageHeader: View {
    let title: String
    let subtitle: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.title2.weight(.bold))
            Text(subtitle)
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
            Text(title)
            if let detail {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct GeneralSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @State private var showingRuntimeMonitor = false

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "通用",
                subtitle: "查看本机服务状态并处理运行问题。"
            )
            Form {
                Section {
                    LabeledContent {
                        Label(runtimeStatusTitle, systemImage: runtimeStatusSymbol)
                            .foregroundStyle(runtimeStatusColor)
                    } label: {
                        SettingsLabel("本机服务", detail: runtimeStatusDetail)
                    }

                    LabeledContent("最近同步") {
                        Text(model.lastSyncAt.map(ZhFormat.syncClock) ?? "尚未同步")
                            .foregroundStyle(.secondary)
                    }

                    if let message = model.runtimeActionMessage {
                        Label(
                            message,
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
        if model.isRestartingRuntime { return "正在重启" }
        return switch model.bridgeStatus {
        case .listening: "运行正常"
        case .starting: "正在启动"
        case .absent: "未连接"
        }
    }

    private var runtimeStatusDetail: String {
        switch model.bridgeStatus {
        case .listening: "Agent 事件与本机控制连接可用"
        case .starting: "正在等待本机服务完成启动"
        case .absent(let reason): reason ?? "本机服务暂时不可用"
        }
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

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "Agent",
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
                        SettingsLabel("Claude 额度", detail: bridgeStatusText)
                    }
                    .disabled(model.isSettingsBusy || model.claudeQuotaBridge?.status == "config_malformed")

                    HStack {
                        SettingsLabel(
                            "主动更新额度",
                            detail: "额度长时间不变或电脑唤醒后可立即请求；若凭证不可用，请先启动 Claude Code CLI 并开始一次会话"
                        )
                        Spacer(minLength: 12)
                        Button(
                            model.isQuotaRefreshBusy ? "正在更新…" : "立即更新",
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
                        Text(message)
                            .font(.caption)
                            .foregroundStyle(
                                message.contains("失败") || message.contains("未连接")
                                    ? Color.red
                                    : Color.secondary
                            )
                            .textSelection(.enabled)
                            .accessibilityLabel("额度更新结果：\(message)")
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
            Button(label) {
                Task { await model.changeSetup(provider: provider, action: action) }
            }
            .buttonStyle(.borderedProminent)
            .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
        } else {
            Button(label) {
                Task { await model.changeSetup(provider: provider, action: action) }
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
        model.showToast("Codex 启动命令已复制；运行后输入 /hooks")
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
    @State private var displayCatalogRevision = 0

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "通知",
                subtitle: "统一管理事件列表、HUD 胶囊与提示音。"
            )
            Form {
                Section {
                    notificationRow(
                        title: "等待批准",
                        detail: "Agent 请求运行命令，等待批准",
                        symbol: "checkmark.shield",
                        kind: "approval"
                    )
                    notificationRow(
                        title: "Agent 提问",
                        detail: "Agent 发出需要回答的问题",
                        symbol: "questionmark.bubble",
                        kind: "question"
                    )
                    notificationRow(
                        title: "出错或卡住",
                        detail: "Agent 报错或长时间没有进展",
                        symbol: "exclamationmark.triangle",
                        kind: "error"
                    )
                    notificationRow(
                        title: "任务完成",
                        detail: "本轮任务完成，等待确认",
                        symbol: "checkmark.circle",
                        kind: "completion"
                    )
                } header: {
                    Text("事件")
                } footer: {
                    Text("“关闭”会从通知列表隐藏该类事件，不改变 Runtime 中的任务事实。")
                }

                Section("声音") {
                    Toggle(isOn: Binding(
                        get: { model.uiSettings.soundEnabled },
                        set: { enabled in model.updateUISettings { $0.soundEnabled = enabled } }
                    )) {
                        SettingsLabel("提示音", detail: "新事件进入列表时播放本机轻提示音")
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
                                Text(display.isMain ? "\(display.name)（主显示器）" : display.name)
                                    .tag(display.id)
                            }
                            if let selectedID = model.hudSettings.selectedDisplayID,
                               resolvedSelectedDisplay == nil
                            {
                                Text("\(model.hudSettings.selectedDisplayName ?? "显示器")（未连接）")
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
        switch model.hudSettings.displayMode {
        case .systemMain:
            "始终显示在 macOS 当前的主显示器"
        case .selectedDisplay:
            "固定显示在下方选择的显示器"
        case .followActRealmWindow:
            "ActRealm 主窗口跨屏移动后，HUD 会同步跟随"
        }
    }

    private var selectedDisplayDetail: String {
        guard resolvedSelectedDisplay != nil else {
            return "所选显示器未连接时暂用系统主显示器"
        }
        return "当前已连接"
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
        LabeledContent {
            Picker("", selection: notificationBinding(kind)) {
                Text("仅列表").tag(NotificationMode.list)
                Text("关闭").tag(NotificationMode.ignore)
            }
            .labelsHidden()
            .frame(width: 120)
        } label: {
            Label {
                SettingsLabel(title, detail: detail)
            } icon: {
                Image(systemName: symbol)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func notificationBinding(_ kind: String) -> Binding<NotificationMode> {
        Binding(
            get: { model.uiSettings.notificationRules.mode(for: kind) == .ignore ? .ignore : .list },
            set: { mode in
                model.updateUISettings { settings in
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
        Toggle(label, isOn: Binding(
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
        ))
        .toggleStyle(.checkbox)
    }

}

private struct ThemeSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @State private var importError: String?

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "主题",
                subtitle: "用自己的图片更换 ActRealm 工作区背景。"
            )
            Form {
                Section {
                    ThemeLanePreview(
                        backgroundURL: model.themeBackgroundURL,
                        backgroundKind: model.themeSettings.backgroundKind,
                        laneOpacity: model.themeSettings.laneOpacity
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
                                ? "\(backgroundKindLabel)已复制到 ActRealm 的本地应用数据目录"
                                : "正在使用默认玻璃背景"
                        )
                        Spacer()
                        if hasCustomBackground {
                            Button("恢复默认", role: .destructive) {
                                model.resetThemeBackground()
                            }
                        }
                        Button("选择图片 / GIF / 视频…", action: chooseBackground)
                            .buttonStyle(.borderedProminent)
                    }
                } header: {
                    Text("背景图片")
                } footer: {
                    Text("支持静态图片、GIF、MP4、MOV 等 macOS 可读取格式。GIF 与视频会静音自动循环；文件只保存在本机。")
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
            Text(importError ?? "请选择另一张图片。")
        }
    }

    private var hasCustomBackground: Bool { model.themeBackgroundURL != nil }

    private var backgroundKindLabel: String {
        switch model.themeSettings.backgroundKind {
        case .image: "静态图片"
        case .animatedImage: "GIF"
        case .video: "循环视频"
        }
    }

    private func chooseBackground() {
        let panel = NSOpenPanel()
        panel.title = "选择 ActRealm 背景"
        panel.prompt = "使用背景"
        panel.allowedContentTypes = [.image, .movie]
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
    let backgroundURL: URL?
    let backgroundKind: ThemeBackgroundKind
    let laneOpacity: Double

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
            AppThemeBackdrop(url: backgroundURL, kind: backgroundKind)
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
            radius: 13,
            stroke: Color.white.opacity(0.55),
            shadow: .clear,
            shadowRadius: 0,
            shadowY: 0
        ))
    }

    private var backgroundLabel: String {
        guard backgroundURL != nil else { return "默认背景" }
        switch backgroundKind {
        case .image: return "静态图片"
        case .animatedImage: return "GIF 循环"
        case .video: return "视频循环 · 静音"
        }
    }
}

private struct DisplaySettingsPage: View {
    @EnvironmentObject private var model: AppModel

    private let presets: [String: [String]] = [
        "concise": ["project", "task", "activity"],
        "detailed": [
            "project", "task", "model", "activity", "plan", "tokens", "cost", "context",
            "tool", "subagents", "environment", "recovery", "control", "jump",
        ],
        "developer": [
            "project", "task", "model", "activity", "plan", "tokens", "cost", "context",
            "tool", "permissionMode", "subagents", "environment", "recovery", "control",
            "jump", "titleSource", "sessionId", "providerSessionId", "providerTurnId", "lastEventAt",
        ],
    ]

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "显示",
                subtitle: "控制任务卡的信息密度；只显示 Runtime 允许的安全字段。"
            )
            Form {
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
                        LazyVGrid(
                            columns: [GridItem(.flexible()), GridItem(.flexible())],
                            alignment: .leading,
                            spacing: 10
                        ) {
                            ForEach(model.displayCatalog) { field in
                                Toggle(field.label, isOn: fieldBinding(field))
                                    .toggleStyle(.checkbox)
                                    .disabled(model.uiSettings.displayProfile != "custom")
                            }
                        }
                        .padding(.vertical, 4)
                    }
                } header: {
                    Text("任务卡字段")
                } footer: {
                    Text("三个预设提供固定字段组合；开启“自定义字段”后可逐项调整。原始提示、命令和文件内容不会因此显示。")
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
}

private struct DataSettingsPage: View {
    @EnvironmentObject private var model: AppModel
    @State private var showingClearConfirmation = false
    @State private var clearConfirmation = ""
    @State private var exporting = false

    var body: some View {
        VStack(spacing: 0) {
            SettingsPageHeader(
                title: "数据",
                subtitle: "管理本机保留、导出和使用统计；ActRealm 不发送遥测。"
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

                    HStack {
                        Button("导出全部数据…") { Task { await export(metricsOnly: false) } }
                            .disabled(exporting || model.isDemo)
                        Button("导出使用统计…") { Task { await export(metricsOnly: true) } }
                            .disabled(exporting || model.isDemo)
                        Spacer()
                    }
                }

                Section {
                    metricsGrid
                } header: {
                    Text("使用统计")
                } footer: {
                    Text("统计只在这台 Mac 上累计。")
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
                                    if await model.clearLocalData(confirmation: clearConfirmation) {
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
                    Text("此操作不可撤销。")
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
        }
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
                metric(model.eventUIP95Ms.map { "\($0)ms" } ?? "—", "界面更新 p95")
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 6)
    }

    private func metric(_ value: String, _ label: String) -> some View {
        VStack(spacing: 3) {
            Text(value)
                .font(.title3.weight(.semibold))
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
    }

    @MainActor
    private func export(metricsOnly: Bool) async {
        guard !exporting else { return }
        exporting = true
        defer { exporting = false }
        guard let data = await model.exportLocalData(metricsOnly: metricsOnly) else { return }
        let panel = NSSavePanel()
        panel.nameFieldStringValue = metricsOnly ? "actrealm-metrics.json" : "actrealm-export.json"
        panel.allowedContentTypes = [.json]
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try data.write(to: url, options: .atomic)
            model.showToast(metricsOnly ? "统计已导出" : "本地数据已导出")
        } catch {
            model.showToast("保存失败：\(error.localizedDescription)")
        }
    }
}
