import AppKit
import ActRealmKit
import SwiftUI

/// Native counterpart of the Web first-run/setup center. All detection and
/// mutations come from `/api/v1/setup`; this view never edits Provider files.
struct AgentSetupView: View {
    @EnvironmentObject private var model: AppModel
    let onBack: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            pageHeader

            VStack(spacing: 0) {
                overview
                Divider().opacity(0.45)
                if let setup = model.setupInfo {
                    ScrollView(.vertical) {
                        LazyVStack(spacing: 12) {
                            ForEach(setup.providers) { provider in
                                ProviderSetupCard(provider: provider)
                            }
                        }
                        .padding(16)
                    }
                } else {
                    ProgressView("正在读取本机 Agent 接入状态…")
                        .controlSize(.small)
                        .foregroundStyle(DT.textSecondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                Divider().opacity(0.45)
                footer
            }
            .liquidGlassSurface(
                tint: DT.cardSoft.opacity(0.2),
                radius: 24,
                stroke: DT.hairline,
                shadow: DT.cardShadow.opacity(0.7),
                shadowRadius: 24,
                shadowY: 10
            )
        }
        .padding(.horizontal, 28)
        .padding(.top, 16)
        .padding(.bottom, 22)
        .task { await model.refreshSetup() }
    }

    private var pageHeader: some View {
        HStack(spacing: 14) {
            Button(action: onBack) {
                Image(systemName: "chevron.left")
                    .font(.system(size: 12, weight: .bold))
                    .frame(width: 30, height: 30)
            }
            .buttonStyle(.plain)
            .background(DT.cardMedium, in: Circle())
            .overlay(Circle().strokeBorder(DT.hairline, lineWidth: 1))

            VStack(alignment: .leading, spacing: 3) {
                Text("AGENT SETUP")
                    .font(.system(size: 10, weight: .heavy))
                    .kerning(1.1)
                    .foregroundStyle(DT.textWeak)
                Text("在一处管理所有 Agent")
                    .font(.system(size: 24, weight: .heavy))
                    .foregroundStyle(DT.textStrong)
                Text("接入状态由本机实时检测；配置写入前自动备份，不需要 ActRealm 账号。")
                    .font(.system(size: 11.5))
                    .foregroundStyle(DT.textSecondary)
            }
            Spacer()
            Label("本机 · 不发送遥测", systemImage: "lock.shield")
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(DT.greenText)
                .padding(.horizontal, 11)
                .padding(.vertical, 5)
                .background(DT.greenBg, in: Capsule())
                .overlay(Capsule().strokeBorder(DT.greenStroke, lineWidth: 1))
        }
    }

    private var overview: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Claude 与 Codex")
                    .font(.system(size: 14, weight: .bold))
                    .foregroundStyle(DT.textStrong)
                Text("安全接入、真实事件验证和 Codex 信任检查都在这里完成。")
                    .font(.system(size: 10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            Chip(text: "已接入 \(model.connectedAgentCount)", tone: .green, fontSize: 10)
            Chip(
                text: "待处理 \(model.pendingAgentSetupCount)",
                tone: model.pendingAgentSetupCount > 0 ? .amber : .neutral,
                fontSize: 10
            )
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
    }

    private var footer: some View {
        HStack(spacing: 12) {
            Label("只展示真实能力；配置冲突时停止写入，不会静默覆盖。", systemImage: "checkmark.shield")
                .font(.system(size: 10.5))
                .foregroundStyle(DT.textWeak)
            Spacer()
            Button("查看接入指南", action: openGuide)
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 10.5, horizontalPadding: 12))
            Button {
                Task { await model.refreshSetup() }
            } label: {
                Label("刷新状态", systemImage: "arrow.clockwise")
            }
            .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 10.5, horizontalPadding: 12))
            .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
    }

    private func openGuide() {
        guard let url = URL(string: "https://github.com/Frontier-Interfaces/ActRealm/blob/agent/v1-full/docs/USER_GUIDE_zh-CN.md") else { return }
        NSWorkspace.shared.open(url)
    }
}

private struct ProviderSetupCard: View {
    @EnvironmentObject private var model: AppModel
    let provider: SetupInfo.ProviderSetup

    private var kind: ProviderKind {
        ProviderKind(record: provider.provider) ?? .codex
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 12) {
                ProviderAvatar(kind: kind, size: 34)
                VStack(alignment: .leading, spacing: 3) {
                    Text(kind == .claude ? "Claude Code" : "Codex")
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(DT.textStrong)
                    Text(provider.detectedText)
                        .font(.system(size: 10.5))
                        .foregroundStyle(DT.textWeak)
                }
                Spacer(minLength: 10)
                statusChip
            }

            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("配置")
                    .font(.system(size: 9.5, weight: .bold))
                    .foregroundStyle(DT.textWeak)
                Text(provider.configPath ?? "尚未生成配置路径")
                    .font(.system(size: 10.5, design: .monospaced))
                    .foregroundStyle(DT.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .textSelection(.enabled)
            }
            .padding(.top, 11)

            Text(statusDetail)
                .font(.system(size: 10.5))
                .foregroundStyle(statusTone.text)
                .padding(.top, 7)

            if provider.provider == "codex", provider.status == "needs_trust" {
                codexTrustSteps
                    .padding(.top, 12)
            }

            HStack(spacing: 8) {
                actions
                Spacer()
                if model.isSetupBusy { ProgressView().controlSize(.small) }
            }
            .padding(.top, 13)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(DT.cardMedium, in: RoundedRectangle(cornerRadius: 17, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 17, style: .continuous)
                .strokeBorder(statusTone.stroke.opacity(0.8), lineWidth: 1)
        )
    }

    @ViewBuilder
    private var actions: some View {
        if provider.canRepair == true {
            actionButton("修复二进制", rank: .primary, action: "repair")
        } else {
            switch provider.status {
            case "not_installed":
                actionButton("安全接入", rank: .primary, action: "install")
            case "needs_reinstall":
                actionButton("检查后重新安装", rank: .primary, action: "install")
            case "needs_trust":
                if provider.reviewCommand != nil {
                    Button("复制信任命令", action: copyTrustCommand)
                        .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 10.5, horizontalPadding: 12))
                }
                refreshButton
                actionButton("移除接入", rank: .destructiveGhost, action: "uninstall")
            case "installed_unverified", "connected":
                refreshButton
                actionButton("移除接入", rank: .destructiveGhost, action: "uninstall")
            case "provider_missing", "cli_missing":
                Button("查看安装说明", action: openGuide)
                    .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 10.5, horizontalPadding: 12))
            default:
                refreshButton
            }
        }
    }

    private func actionButton(
        _ label: String,
        rank: PillButtonStyle.Rank,
        action: String
    ) -> some View {
        Button(label) {
            Task { await model.changeSetup(provider: provider.provider, action: action) }
        }
        .buttonStyle(PillButtonStyle(rank: rank, fontSize: 10.5, horizontalPadding: 12))
        .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
    }

    private var refreshButton: some View {
        Button("刷新状态") { Task { await model.refreshSetup() } }
            .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 10.5, horizontalPadding: 12))
            .disabled(!model.bridgeStatus.isListening || model.isSetupBusy)
    }

    private var codexTrustSteps: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Codex 信任必须在官方界面确认")
                .font(.system(size: 10.5, weight: .bold))
                .foregroundStyle(DT.amberText)
            Text("1. \(codexStartStep)   2. 输入 /hooks   3. 核对命令路径并信任   4. 新建会话后刷新")
                .font(.system(size: 10))
                .foregroundStyle(DT.textSecondary)
            if let command = provider.reviewCommand {
                Text(command)
                    .font(.system(size: 9.5, design: .monospaced))
                    .foregroundStyle(DT.textWeak)
                    .textSelection(.enabled)
                    .lineLimit(2)
            }
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(DT.amberBg.opacity(0.55), in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(DT.amberStroke, lineWidth: 1))
    }

    private var codexStartStep: String {
        if provider.cliInstalled == true { return "打开任意 Codex 终端会话" }
        return "在终端运行卡片中的内置 Codex 命令"
    }

    private var statusChip: some View {
        Chip(text: statusLabel, tone: statusTone.chip, fontSize: 10)
    }

    private var statusLabel: String {
        switch provider.status {
        case "connected": "已接入"
        case "installed_unverified": "等待验证"
        case "needs_trust": "等待信任"
        case "needs_reinstall": "配置有变化"
        case "not_installed": "未接入"
        case "provider_missing", "cli_missing": "未找到客户端"
        case "inline_conflict": "配置冲突"
        case "error": "配置无法解析"
        default: provider.status
        }
    }

    private var statusDetail: String {
        switch provider.status {
        case "connected": "已收到安装后的真实 Agent 事件，实时活动可以正常显示。"
        case "installed_unverified": "配置已经就绪；启动一次真实会话后才能确认接入。"
        case "needs_trust": "打开 Codex，输入 /hooks，逐项检查并信任 ActRealm。"
        case "needs_reinstall": "发现不完整或被修改的 ActRealm 条目；不会自动覆盖。"
        case "not_installed": "点击后先备份，再语义合并；不会静默替换现有配置。"
        case "provider_missing", "cli_missing": "请先安装该 Agent 的桌面客户端或命令行程序。"
        case "inline_conflict": "Codex 同时存在 inline Hook；请先保留一种同层配置形式。"
        case "error": "为保护设置，ActRealm 已拒绝改写；请先恢复或修正配置。"
        default: provider.statusText
        }
    }

    private var statusTone: (chip: Chip.Tone, text: Color, stroke: Color) {
        switch provider.status {
        case "connected": (.green, DT.greenText, DT.greenStroke)
        case "installed_unverified", "needs_trust": (.amber, DT.amberText, DT.amberStroke)
        case "needs_reinstall", "provider_missing", "cli_missing", "inline_conflict", "error":
            (.red, DT.redText, DT.redStroke)
        default: (.neutral, DT.textWeak, DT.hairline)
        }
    }

    private func copyTrustCommand() {
        guard let command = provider.reviewCommand else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(command, forType: .string)
        model.showToast("Codex 启动命令已复制；运行后输入 /hooks")
    }

    private func openGuide() {
        guard let url = URL(string: "https://github.com/Frontier-Interfaces/ActRealm/blob/agent/v1-full/docs/USER_GUIDE_zh-CN.md") else { return }
        NSWorkspace.shared.open(url)
    }
}
