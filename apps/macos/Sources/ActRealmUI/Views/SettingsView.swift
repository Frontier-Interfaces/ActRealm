import ActRealmKit
import SwiftUI

public struct SettingsView: View {
    public init() {}

    @EnvironmentObject var model: AppModel
    @Environment(\.snapshotRendering) private var snapshotRendering

    public var body: some View {
        Group {
            if snapshotRendering {
                VStack(spacing: 0) {
                    HStack(spacing: 8) {
                        snapshotTab("General", systemImage: "gearshape", selected: true)
                        snapshotTab("Providers", systemImage: "person.2.badge.gearshape", selected: false)
                        Spacer()
                    }
                    .padding(.horizontal, 18)
                    .padding(.top, 14)
                    GeneralSettingsTab()
                }
            } else {
                TabView {
                    GeneralSettingsTab()
                        .tabItem { Label("General", systemImage: "gearshape") }
                    ProvidersSettingsTab()
                        .tabItem { Label("Providers", systemImage: "person.2.badge.gearshape") }
                }
            }
        }
        .frame(width: 560)
        .frame(minHeight: 420)
        .background {
            if snapshotRendering {
                Rectangle().fill(.ultraThinMaterial)
            } else {
                WindowGlass().ignoresSafeArea()
            }
        }
    }

    private func snapshotTab(_ title: String, systemImage: String, selected: Bool) -> some View {
        Label(title, systemImage: systemImage)
            .font(.system(size: 11.5, weight: selected ? .semibold : .regular))
            .foregroundStyle(selected ? DT.textPrimary : DT.textWeak)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(selected ? DT.cardStrong : Color.clear, in: Capsule())
    }
}

// MARK: - Group card chrome

private struct SettingsGroup<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.system(size: 10.5, weight: .bold))
                .kerning(0.6)
                .foregroundStyle(Color(lightWhite: 0.4, darkWhite: 0.45))
                .padding(.horizontal, 14)
                .padding(.top, 10)
                .padding(.bottom, 2)
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .sheetCard(
            fill: DT.cardMedium,
            stroke: DT.hairline,
            radius: DT.radiusCard,
            shadow: DT.softShadow,
            shadowRadius: 3,
            shadowY: 1
        )
    }
}

private struct GroupDivider: View {
    var body: some View {
        Rectangle()
            .fill(Color(lightWhite: 0.05, darkWhite: 0.08))
            .frame(height: 1)
            .padding(.leading, 14)
    }
}

// MARK: - General

private struct GeneralSettingsTab: View {
    @EnvironmentObject var model: AppModel
    @State private var showingRuntimeMonitor = false

    var body: some View {
        VStack(spacing: 12) {
            SettingsGroup(title: "APPROVAL") {
                approvalRow(
                    .prompt,
                    subtitle: "Ask for every PermissionRequest"
                )
                GroupDivider()
                approvalRow(.allowAll, subtitle: "自动允许全部请求（每条仍有 3 秒撤回窗口）")
                GroupDivider()
                approvalRow(
                    .denyAll,
                    subtitle: "Replies \"User denied the permission request\""
                )
            }
            SettingsGroup(title: "RUNTIME") {
                runtimeControlRow
                GroupDivider()
                runtimeRow(label: "Socket", value: "~/.flow-agent/run/bridge.sock")
                GroupDivider()
                runtimeRow(label: "Data folder", value: "~/.flow-agent")
                if let message = model.runtimeActionMessage {
                    GroupDivider()
                    Text(message)
                        .font(DT.body(10.5))
                        .foregroundStyle(model.bridgeStatus.isListening ? DT.greenText : DT.redText)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 8)
                }
            }
            Text("Local-first — no telemetry, no cloud backend.")
                .font(DT.micro(10.5))
                .foregroundStyle(DT.textWeak)
                .frame(maxWidth: .infinity)
                .padding(.top, 2)
        }
        .padding(18)
        .sheet(isPresented: $showingRuntimeMonitor) {
            RuntimeMonitorView()
                .environmentObject(model)
        }
    }

    private var runtimeControlRow: some View {
        HStack(spacing: 9) {
            StatusDot(
                color: model.bridgeStatus.isListening ? DT.greenDot : DT.redText,
                size: 7,
                glow: model.bridgeStatus.isListening
            )
            VStack(alignment: .leading, spacing: 1) {
                Text(model.bridgeStatus.isListening ? "本机 Runtime 在线" : "Runtime 未连接")
                    .font(.system(size: 12.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                Text("Hook 事件与审批命令通过本地 bridge.sock 传递")
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            Button("查看监控") {
                showingRuntimeMonitor = true
            }
            .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 10.5, horizontalPadding: 12))
            Button(model.isRestartingRuntime ? "正在重启…" : "重启") {
                model.restartRuntime()
            }
            .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 10.5, horizontalPadding: 12))
            .disabled(model.isDemo || model.isRestartingRuntime)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }

    private func approvalRow(_ policy: ApprovalPolicy, subtitle: String?) -> some View {
        Button {
            model.approvalPolicy = policy
        } label: {
            HStack(spacing: 10) {
                ZStack {
                    if model.approvalPolicy == policy {
                        Circle().fill(DT.blue).frame(width: 16, height: 16)
                        Circle().fill(.white).frame(width: 6, height: 6)
                    } else {
                        Circle()
                            .strokeBorder(Color(lightWhite: 0.25, darkWhite: 0.35), lineWidth: 1.5)
                            .frame(width: 16, height: 16)
                    }
                }
                VStack(alignment: .leading, spacing: 1) {
                    Text(policy.title)
                        .font(.system(size: 12.5, weight: model.approvalPolicy == policy ? .semibold : .regular))
                        .foregroundStyle(Color(lightWhite: 0.85, darkWhite: 0.9))
                    if let subtitle {
                        Text(subtitle)
                            .font(DT.body(10.5))
                            .foregroundStyle(DT.textWeak)
                    }
                }
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func runtimeRow(label: String, value: String) -> some View {
        HStack {
            Text(label)
                .font(DT.body(12.5))
                .foregroundStyle(Color(lightWhite: 0.8, darkWhite: 0.88))
            Spacer()
            Text(value)
                .font(DT.mono(11))
                .foregroundStyle(Color(lightWhite: 0.5, darkWhite: 0.55))
                .textSelection(.enabled)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }
}

// MARK: - Providers

private struct ProvidersSettingsTab: View {
    @EnvironmentObject var model: AppModel
    @State private var setup: SetupInfo?
    @State private var busyProvider: String?
    @State private var errorMessage: String?

    var body: some View {
        VStack(spacing: 12) {
            SettingsGroup(title: "PROVIDERS") {
                providerRow(.claude, fallbackSubtitle: "Hook 未安装 · reply window 24h")
                GroupDivider()
                providerRow(.codex, fallbackSubtitle: "Hook 未安装 · reply window 1h")
                GroupDivider()
                geminiRow
            }
            if model.isDemo {
                Text("演示模式：Provider 安装操作不可用")
                    .font(DT.micro(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Text("安装会先备份现有配置；Codex 需要你在其 /hooks 界面手动信任。")
                .font(DT.micro(10.5))
                .foregroundStyle(DT.textWeak)
                .frame(maxWidth: .infinity)
        }
        .padding(18)
        .task { await refresh() }
        .alert("Provider 设置失败", isPresented: .init(
            get: { errorMessage != nil },
            set: { if !$0 { errorMessage = nil } }
        )) {
            Button("好", role: .cancel) {}
        } message: {
            Text(errorMessage ?? "")
        }
    }

    private func refresh() async {
        guard !model.isDemo else { return }
        setup = await model.client.fetchSetup()
    }

    private func providerSetup(_ kind: ProviderKind) -> SetupInfo.ProviderSetup? {
        setup?.providers.first { $0.provider == kind.rawValue }
    }

    private func providerRow(_ kind: ProviderKind, fallbackSubtitle: String) -> some View {
        let info = providerSetup(kind)
        let replyWindow = kind == .claude ? "reply window 24h" : "reply window 1h"
        let subtitle = info.map { "\($0.statusText) · \(replyWindow)" } ?? fallbackSubtitle
        return HStack(spacing: 10) {
            ProviderAvatar(kind: kind, size: 22)
            VStack(alignment: .leading, spacing: 1) {
                Text(kind.displayName)
                    .font(.system(size: 12.5, weight: .semibold))
                    .foregroundStyle(Color(lightWhite: 0.85, darkWhite: 0.9))
                Text(subtitle)
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            if busyProvider == kind.rawValue {
                ProgressView().controlSize(.small)
            }
            Toggle("", isOn: toggleBinding(kind, info: info))
                .toggleStyle(.switch)
                .controlSize(.small)
                .labelsHidden()
                .disabled(model.isDemo || busyProvider != nil || setup == nil)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }

    private func toggleBinding(_ kind: ProviderKind, info: SetupInfo.ProviderSetup?) -> Binding<Bool> {
        Binding(
            get: { info?.isInstalled ?? false },
            set: { enable in
                Task { await change(kind, action: enable ? "install" : "uninstall") }
            }
        )
    }

    private func change(_ kind: ProviderKind, action: String) async {
        busyProvider = kind.rawValue
        defer { busyProvider = nil }
        let (updated, error) = await model.client.changeSetup(provider: kind.rawValue, action: action)
        if let updated {
            setup = updated
        }
        if let error {
            errorMessage = error
        }
    }

    private var geminiRow: some View {
        HStack(spacing: 10) {
            ProviderAvatar(kind: .gemini, size: 22)
            VStack(alignment: .leading, spacing: 1) {
                Text("gemini")
                    .font(.system(size: 12.5, weight: .semibold))
                    .foregroundStyle(Color(lightWhite: 0.85, darkWhite: 0.9))
                Text("Notify-only in v1 · acknowledged in 200ms")
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            Text("v1 不接入")
                .font(DT.micro(10))
                .foregroundStyle(DT.textFaint)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
    }
}
