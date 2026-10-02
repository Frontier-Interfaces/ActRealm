import ActRealmKit
import SwiftUI

/// Runtime health, ownership, and recovery UI opened from the main window.
/// It deliberately separates "a process exists" from "the app is connected".
public struct RuntimeMonitorView: View {
    public init() {}

    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.snapshotRendering) private var snapshotRendering
    @Environment(\.locale) private var locale
    @State private var showingTechnicalDetails = false

    public var body: some View {
        VStack(spacing: 0) {
            header
            if snapshotRendering {
                monitorContent
            } else {
                ScrollView {
                    monitorContent
                }
            }
            actionBar
        }
        .frame(
            minWidth: 640,
            idealWidth: 780,
            maxWidth: 900,
            minHeight: snapshotRendering ? 1_040 : 560,
            idealHeight: snapshotRendering ? 1_040 : 760,
            maxHeight: snapshotRendering ? 1_040 : 880
        )
        .modifier(WindowGlassBackground())
        .task {
            await model.refreshControlPlaneDiagnostics(includeDoctor: true)
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(5))
                guard !Task.isCancelled else { return }
                await model.refreshControlPlaneDiagnostics(includeDoctor: false)
            }
        }
    }

    private var monitorContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            healthSummary
            if let warning = model.runtimeDiagnostics.launchAgentWarning {
                warningCard(warning)
            }
            ControlPlaneDiagnosticsView()
                .environmentObject(model)
            DisclosureGroup(
                isExpanded: $showingTechnicalDetails
            ) {
                VStack(alignment: .leading, spacing: 12) {
                    processDetails
                    logPanel
                }
                .padding(.top, 10)
            } label: {
                Label(
                    localized("技术详情与最近 Runtime 输出", locale: locale),
                    systemImage: "wrench.and.screwdriver"
                )
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textWeak)
            }
            .padding(12)
            .liquidGlassSurface(
                tint: DT.cardFaint.opacity(0.12),
                radius: 14,
                stroke: DT.hairlineSoft
            )
        }
        .padding(18)
    }

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: "server.rack")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(DT.logoTint)
                .frame(width: 32, height: 32)
                .liquidGlassSurface(
                    tint: DT.logoTint.opacity(0.12),
                    radius: 999,
                    stroke: DT.logoTint.opacity(0.16)
                )
            VStack(alignment: .leading, spacing: 1) {
                Text("Runtime 状态与诊断")
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                Text("进程存在不代表服务可用；这里同时检查连接、锁和 Bridge。")
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer()
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 10, weight: .bold))
                    .frame(width: 24, height: 24)
            }
            .buttonStyle(PillButtonStyle(rank: .tertiary, horizontalPadding: 6))
            .accessibilityLabel(localized("关闭 Runtime 监控", locale: locale))
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 13)
        .overlay(alignment: .bottom) {
            Rectangle().fill(DT.separator).frame(height: 1)
        }
    }

    private var healthSummary: some View {
        HStack(spacing: 14) {
            ZStack {
                Circle()
                    .fill(statusColor.opacity(0.14))
                    .frame(width: 54, height: 54)
                Image(systemName: statusSymbol)
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(statusColor)
            }
            VStack(alignment: .leading, spacing: 3) {
                Text(statusTitle)
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(DT.textPrimary)
                Text(statusDetail)
                    .font(DT.body(11))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(2)
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 4) {
                Text("最近检查")
                    .font(DT.micro(9.5))
                    .foregroundStyle(DT.textFaint)
                Text(checkTime)
                    .font(DT.mono(10.5))
                    .foregroundStyle(DT.textWeak)
            }
        }
        .padding(16)
        .liquidGlassSurface(
            tint: statusColor.opacity(0.07),
            radius: 18,
            stroke: statusColor.opacity(0.2),
            shadow: DT.softShadow,
            shadowRadius: 5,
            shadowY: 2
        )
    }

    private func warningCard(_ warning: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(DT.amberText)
            VStack(alignment: .leading, spacing: 3) {
                Text("发现旧的后台启动项")
                    .font(.system(size: 11.5, weight: .bold))
                    .foregroundStyle(DT.amberTextSoft)
                Text(AppLocalization.localizedRuntimeSupervisorText(
                    warning,
                    language: model.appLanguage
                ))
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .liquidGlassSurface(
            tint: DT.amberBg.opacity(0.24),
            radius: 14,
            stroke: DT.amberStroke
        )
    }

    private var processDetails: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle("SERVICE DETAILS")
            LazyVGrid(
                columns: [GridItem(.flexible()), GridItem(.flexible())],
                spacing: 8
            ) {
                detailCell(
                    title: "控制连接",
                    value: model.bridgeStatus.isListening ? "已连接" : "未连接",
                    symbol: "point.3.connected.trianglepath.dotted",
                    tone: model.bridgeStatus.isListening ? DT.greenDot : DT.redText
                )
                detailCell(
                    title: "管理进程",
                    value: pidText(model.runtimeDiagnostics.managedPID),
                    symbol: "cpu",
                    tone: model.runtimeDiagnostics.managedPID == nil ? DT.textWeak : DT.greenDot
                )
                detailCell(
                    title: "runtime.lock",
                    value: lockOwnerText,
                    symbol: "lock",
                    tone: model.runtimeDiagnostics.lockOwnerIsAlive ? DT.greenDot : DT.textWeak
                )
                detailCell(
                    title: "bridge.sock",
                    value: model.runtimeDiagnostics.socketExists ? "存在" : "缺失",
                    symbol: "cable.connector",
                    tone: model.runtimeDiagnostics.socketExists ? DT.greenDot : DT.redText
                )
                detailCell(
                    title: "Codex 连接器",
                    value: codexConnectorStatus,
                    symbol: "point.3.connected.trianglepath.dotted",
                    tone: codexConnectorTone
                )
                detailCell(
                    title: "最近 Provider 通知",
                    value: codexLastNotification,
                    symbol: "clock.arrow.circlepath",
                    tone: codexLastNotificationTone
                )
            }

            detailRow(title: "Endpoint", value: model.runtimeDiagnostics.endpoint ?? "尚未建立")
            detailRow(title: "Helper", value: model.runtimeDiagnostics.helperPath ?? "未找到")
            if let ownerPath = model.runtimeDiagnostics.lockOwnerPath {
                detailRow(title: "锁持有者", value: ownerPath)
            }
            if let diagnostic = codexPlanDiagnostic {
                detailRow(title: "计划诊断", value: diagnostic)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func detailCell(title: String, value: String, symbol: String, tone: Color) -> some View {
        HStack(spacing: 9) {
            Image(systemName: symbol)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(tone)
                .frame(width: 22, height: 22)
                .background(tone.opacity(0.1), in: RoundedRectangle(cornerRadius: 7))
            VStack(alignment: .leading, spacing: 1) {
                Text(localized(title, locale: locale))
                    .font(DT.micro(9.5))
                    .foregroundStyle(DT.textFaint)
                Text(localized(value, locale: locale))
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .liquidGlassSurface(tint: DT.cardFaint.opacity(0.18), radius: 12, stroke: DT.hairlineSoft)
    }

    private func detailRow(title: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(localized(title, locale: locale))
                .font(DT.micro(9.5))
                .foregroundStyle(DT.textFaint)
                .frame(width: 74, alignment: .leading)
            Text(localized(value, locale: locale))
                .font(DT.mono(10))
                .foregroundStyle(DT.textWeak)
                .lineLimit(1)
                .truncationMode(.middle)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }

    private var logPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("RECENT RUNTIME OUTPUT")
            Group {
                if snapshotRendering {
                    logTextView
                } else {
                    ScrollView {
                        logTextView
                    }
                }
            }
            .frame(height: 112)
            .background(Color.black.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(DT.hairlineSoft, lineWidth: 1)
            )
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var logTextView: some View {
        Text(logText)
            .font(.system(size: 9.5, design: .monospaced))
            .foregroundStyle(Color(lightWhite: 0.72, darkWhite: 0.75))
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .padding(10)
    }

    private var actionBar: some View {
        HStack(spacing: 10) {
            if let message = model.runtimeActionMessage {
                Text(AppLocalization.localizedRuntimeSupervisorText(
                    message,
                    language: model.appLanguage
                ))
                    .font(DT.body(10.5))
                    .foregroundStyle(model.bridgeStatus.isListening ? DT.greenText : DT.redText)
                    .lineLimit(2)
            } else {
                Text("重启会先校验 runtime.lock 持有者路径，再停止旧进程。")
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textFaint)
            }
            Spacer()
            Button("预览 HUD") {
                model.previewHUD()
                dismiss()
            }
            .buttonStyle(PillButtonStyle(rank: .tertiary, fontSize: 10.5, horizontalPadding: 12))
            Button {
                Task {
                    await model.refreshControlPlaneDiagnostics(includeDoctor: true)
                }
            } label: {
                HStack(spacing: 6) {
                    if model.isRefreshingControlPlaneDiagnostics {
                        ProgressView().controlSize(.small)
                    }
                    Text(localized("重新检查", locale: locale))
                }
            }
            .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 10.5, horizontalPadding: 13))
            .disabled(model.isRefreshingControlPlaneDiagnostics)
            Button {
                model.restartRuntime()
            } label: {
                HStack(spacing: 6) {
                    if model.isRestartingRuntime {
                        ProgressView().controlSize(.small)
                    } else {
                        Image(systemName: "arrow.clockwise")
                    }
                    Text(localized(
                        model.isRestartingRuntime ? "正在重启…" : "重启 Runtime",
                        locale: locale
                    ))
                }
            }
            .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 10.5, horizontalPadding: 14))
            .disabled(model.isDemo || model.isRestartingRuntime)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
        .overlay(alignment: .top) {
            Rectangle().fill(DT.separator).frame(height: 1)
        }
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 9.5, weight: .bold))
            .kerning(0.55)
            .foregroundStyle(DT.textFaint)
    }

    private var statusTitle: String {
        if model.isDemo {
            return localized("Runtime 监控预览", locale: locale)
        }
        if model.isRestartingRuntime {
            return localized("正在重启 Runtime", locale: locale)
        }
        if model.controlPlaneDiagnosticsError != nil,
           model.bridgeStatus.isListening
        {
            return localized("Runtime 在线 · 诊断延迟", locale: locale)
        }
        let key = switch model.bridgeStatus {
        case .listening: "Runtime 在线"
        case .starting: "Runtime 正在启动"
        case .absent: "Runtime 未连接"
        }
        return localized(key, locale: locale)
    }

    private var statusDetail: String {
        if model.isDemo {
            return localized(
                "正式运行时会显示真实进程、锁、Bridge 与连接状态。",
                locale: locale
            )
        }
        if let error = model.controlPlaneDiagnosticsError,
           model.bridgeStatus.isListening
        {
            return error
        }
        let key = switch model.bridgeStatus {
        case .listening:
            "控制连接可用，Hook 事件可以进入主界面。"
        case .starting:
            "Helper 已进入启动流程，正在等待 Bootstrap 与快照。"
        case .absent(let reason):
            reason ?? "没有可用的 Runtime 控制连接。"
        }
        return localized(key, locale: locale)
    }

    private var statusColor: Color {
        if model.isDemo { return DT.blue }
        if model.isRestartingRuntime { return DT.blue }
        if model.controlPlaneDiagnosticsError != nil,
           model.bridgeStatus.isListening
        {
            return DT.amberDot
        }
        switch model.bridgeStatus {
        case .listening: return DT.greenDot
        case .starting: return DT.amberDot
        case .absent: return DT.redText
        }
    }

    private var statusSymbol: String {
        if model.isRestartingRuntime { return "arrow.clockwise" }
        return model.bridgeStatus.isListening ? "checkmark.circle.fill" : "exclamationmark.circle.fill"
    }

    private var checkTime: String {
        let date = model.runtimeDiagnostics.checkedAt
        guard date != .distantPast else { return "—" }
        return ZhFormat.syncClock(date)
    }

    private var codexConnector: CodexConnectorCapability? {
        model.client.snapshot.capabilities?.codexConnector
    }

    private var codexConnectorStatus: String {
        switch codexConnector?.status {
        case "connected":
            localized("已连接", locale: locale)
        case "disabled":
            localized("未启用", locale: locale)
        case "unavailable":
            localized("不可用", locale: locale)
        case let status?:
            status
        case nil:
            localized("尚无数据", locale: locale)
        }
    }

    private var codexConnectorTone: Color {
        codexConnector?.status == "connected" ? DT.greenDot : DT.textWeak
    }

    private var codexLastNotification: String {
        guard let method = codexConnector?.lastNotificationMethod,
              !method.isEmpty else {
            return localized("未收到", locale: locale)
        }
        guard let millis = codexConnector?.lastNotificationAt else {
            return method
        }
        let time = ZhFormat.syncClock(Date(timeIntervalSince1970: Double(millis) / 1_000))
        return "\(method) · \(time)"
    }

    private var codexLastNotificationTone: Color {
        codexConnector?.lastNotificationMethod == nil ? DT.textWeak : DT.greenDot
    }

    private var codexPlanDiagnostic: String? {
        switch codexConnector?.lastPlanSkipReason {
        case "missing_thread_id":
            return localized("计划通知缺少 threadId", locale: locale)
        case "missing_plan_array":
            let fields = codexConnector?.lastPlanFieldKeys?.joined(separator: ", ") ?? ""
            let base = localized("计划通知缺少步骤数组", locale: locale)
            return fields.isEmpty ? base : "\(base) · \(fields)"
        case let reason?:
            return reason
        case nil:
            return nil
        }
    }

    private var lockOwnerText: String {
        guard let pid = model.runtimeDiagnostics.lockOwnerPID else {
            return localized("无人持有", locale: locale)
        }
        return model.runtimeDiagnostics.lockOwnerIsAlive
            ? "PID \(pid)"
            : localizedFormat("陈旧 PID %lld", locale: locale, Int64(pid))
    }

    private func pidText(_ pid: Int32?) -> String {
        pid.map { "PID \($0)" } ?? localized("未运行", locale: locale)
    }

    private var logText: String {
        let stdout = model.runtimeDiagnostics.stdoutTail.trimmingCharacters(in: .whitespacesAndNewlines)
        let stderr = model.runtimeDiagnostics.stderrTail.trimmingCharacters(in: .whitespacesAndNewlines)
        let combined = [stdout, stderr]
            .filter { !$0.isEmpty }
            .joined(separator: "\n")
            .split(separator: "\n", omittingEmptySubsequences: false)
            .map {
                AppLocalization.localizedRuntimeSupervisorText(
                    String($0),
                    language: model.appLanguage
                )
            }
            .joined(separator: "\n")
        return combined.isEmpty
            ? localized("等待 Runtime 输出…", locale: locale)
            : combined
    }
}
