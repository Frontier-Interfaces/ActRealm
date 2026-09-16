import ActRealmKit
import SwiftUI

struct ControlPlaneDiagnosticsView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.locale) private var locale
    @State private var showingTechnicalDetails = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            layerSection
            DisclosureGroup(isExpanded: $showingTechnicalDetails) {
                VStack(alignment: .leading, spacing: 14) {
                    identityGrid
                    providerSection
                    conditionalSection
                }
                .padding(.top, 10)
            } label: {
                VStack(alignment: .leading, spacing: 2) {
                    Text(localized("技术与能力详情", locale: locale))
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(DT.textPrimary)
                    Text(localized(
                        "版本、实例、Provider 能力与暂停功能默认折叠",
                        locale: locale
                    ))
                        .font(DT.micro(9))
                        .foregroundStyle(DT.textFaint)
                }
            }
            .tint(DT.textWeak)
            .padding(11)
            .liquidGlassSurface(
                tint: DT.cardFaint.opacity(0.14),
                radius: 13,
                stroke: DT.hairlineSoft
            )
        }
    }

    private var identityGrid: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("CONTROL PLANE FACTS")
            LazyVGrid(
                columns: [GridItem(.flexible()), GridItem(.flexible())],
                spacing: 8
            ) {
                factCell(
                    title: "Runtime",
                    value: runtimeVersion,
                    detail: runtimeCommit,
                    symbol: "server.rack"
                )
                factCell(
                    title: "协议与实例",
                    value: protocolText,
                    detail: instanceText,
                    symbol: "point.3.connected.trianglepath.dotted"
                )
                factCell(
                    title: "Snapshot",
                    value: snapshotRevision,
                    detail: snapshotFreshness,
                    symbol: "waveform.path.ecg"
                )
                factCell(
                    title: "SQLite",
                    value: storageSchema,
                    detail: storageIntegrity,
                    symbol: "cylinder.split.1x2"
                )
            }
        }
    }

    private var layerSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                sectionTitle("DIAGNOSTIC LAYERS")
                Spacer()
                Text(localized("每层只显示自己的事实与恢复边界", locale: locale))
                    .font(DT.micro(9))
                    .foregroundStyle(DT.textFaint)
            }
            VStack(spacing: 0) {
                ForEach(Array(layers.enumerated()), id: \.element.id) { index, layer in
                    layerRow(layer)
                    if index < layers.count - 1 {
                        Rectangle().fill(DT.separator).frame(height: 1)
                    }
                }
            }
            .liquidGlassSurface(
                tint: DT.cardFaint.opacity(0.16),
                radius: 16,
                stroke: DT.hairlineSoft
            )
        }
    }

    private var providerSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle("PROVIDER SURFACES")
            HStack(alignment: .top, spacing: 8) {
                providerCard("claude", title: "Claude Code", symbol: "c.circle.fill")
                providerCard("codex", title: "Codex", symbol: "terminal.fill")
                unsupportedCoworkCard
            }
        }
    }

    private var conditionalSection: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "pause.circle")
                .foregroundStyle(DT.textWeak)
            VStack(alignment: .leading, spacing: 2) {
                Text(localized("条件层不计为当前故障", locale: locale))
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                Text(localized(
                    "诊断仅覆盖本机 Runtime、Agent 接入和伴生应用。",
                    locale: locale
                ))
                    .font(DT.body(9.5))
                    .foregroundStyle(DT.textWeak)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(11)
        .liquidGlassSurface(
            tint: DT.cardFaint.opacity(0.14),
            radius: 13,
            stroke: DT.hairlineSoft
        )
    }

    private func factCell(
        title: String,
        value: String,
        detail: String,
        symbol: String
    ) -> some View {
        HStack(spacing: 9) {
            Image(systemName: symbol)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(DT.blue)
                .frame(width: 25, height: 25)
                .background(DT.blue.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
            VStack(alignment: .leading, spacing: 2) {
                Text(localized(title, locale: locale))
                    .font(DT.micro(9))
                    .foregroundStyle(DT.textFaint)
                Text(localized(value, locale: locale))
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                    .lineLimit(1)
                Text(localized(detail, locale: locale))
                    .font(DT.mono(8.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .liquidGlassSurface(
            tint: DT.cardFaint.opacity(0.16),
            radius: 12,
            stroke: DT.hairlineSoft
        )
    }

    private func layerRow(_ layer: DiagnosticLayerPresentation) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: layer.symbol)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(layer.tone.color)
                .frame(width: 25, height: 25)
                .background(
                    layer.tone.color.opacity(0.1),
                    in: RoundedRectangle(cornerRadius: 8)
                )
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 7) {
                    Text(localized(layer.title, locale: locale))
                        .font(.system(size: 10.5, weight: .semibold))
                        .foregroundStyle(DT.textPrimary)
                    statusBadge(layer.status, tone: layer.tone)
                }
                Text(layer.detail)
                    .font(DT.body(9.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(2)
                Text(layer.metadata)
                    .font(DT.mono(8.3))
                    .foregroundStyle(DT.textFaint)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 0)
            if recoveryAvailable(for: layer) {
                Button(localized(layer.recovery, locale: locale)) {
                    runRecovery(for: layer.id)
                }
                .buttonStyle(PillButtonStyle(
                    rank: .tertiary,
                    fontSize: 8.8,
                    horizontalPadding: 9
                ))
                .disabled(model.isRefreshingControlPlaneDiagnostics)
                .frame(width: 122, alignment: .trailing)
            } else {
                Text(localized(layer.recovery, locale: locale))
                    .font(DT.micro(8.8))
                    .foregroundStyle(DT.textFaint)
                    .multilineTextAlignment(.trailing)
                    .frame(width: 112, alignment: .trailing)
            }
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 9)
    }

    private func providerCard(
        _ provider: String,
        title: String,
        symbol: String
    ) -> some View {
        let setup = model.setupInfo?.providers.first { $0.provider == provider }
        let tone = providerTone(setup?.status)
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 7) {
                Image(systemName: symbol)
                    .foregroundStyle(tone.color)
                Text(title)
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                Spacer(minLength: 0)
                statusBadge(providerStatus(setup?.status), tone: tone)
            }
            Text(providerVersion(provider))
                .font(DT.mono(8.7))
                .foregroundStyle(DT.textWeak)
                .lineLimit(1)
                .truncationMode(.middle)
            Text(providerCapabilitySummary(provider))
                .font(DT.body(9))
                .foregroundStyle(DT.textWeak)
                .lineLimit(2)
            Text(providerSource(provider))
                .font(DT.mono(8))
                .foregroundStyle(DT.textFaint)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .liquidGlassSurface(
            tint: tone.color.opacity(0.05),
            radius: 12,
            stroke: tone.color.opacity(0.16)
        )
    }

    private var unsupportedCoworkCard: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 7) {
                Image(systemName: "person.crop.rectangle.stack")
                    .foregroundStyle(DT.textWeak)
                Text("Claude Cowork")
                    .font(.system(size: 10.5, weight: .semibold))
                    .foregroundStyle(DT.textPrimary)
                Spacer(minLength: 0)
                statusBadge("未支持", tone: .neutral)
            }
            Text(localized("未启用 Connector", locale: locale))
                .font(DT.mono(8.7))
                .foregroundStyle(DT.textWeak)
            Text(localized("没有可验证的任务生命周期事件源，不显示虚假连接或完成。", locale: locale))
                .font(DT.body(9))
                .foregroundStyle(DT.textWeak)
                .lineLimit(2)
            Text("source: no_verified_event_source")
                .font(DT.mono(8))
                .foregroundStyle(DT.textFaint)
                .lineLimit(1)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .liquidGlassSurface(
            tint: DT.cardFaint.opacity(0.12),
            radius: 12,
            stroke: DT.hairlineSoft
        )
    }

    private func statusBadge(_ text: String, tone: DiagnosticTone) -> some View {
        Text(localized(text, locale: locale))
            .font(.system(size: 8.2, weight: .bold))
            .foregroundStyle(tone.color)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(tone.color.opacity(0.1), in: Capsule())
    }

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 9.5, weight: .bold))
            .kerning(0.55)
            .foregroundStyle(DT.textFaint)
    }

    private var runtimeVersion: String {
        guard let status = model.runtimeStatus else { return "等待 Runtime 事实" }
        return "v\(status.version) · PID \(status.pid)"
    }

    private var runtimeCommit: String {
        guard let commit = model.runtimeStatus?.commit, commit != "unknown" else {
            return "commit unavailable in this build"
        }
        return "commit \(String(commit.prefix(12)))"
    }

    private var protocolText: String {
        guard let status = model.runtimeStatus else { return "尚无数据" }
        return "API v\(status.schemaVersion) · Companion v\(status.protocolVersion ?? 0)"
    }

    private var instanceText: String {
        guard let id = model.runtimeStatus?.instanceId else { return "instance unavailable" }
        return "instance \(String(id.prefix(12)))"
    }

    private var snapshotRevision: String {
        guard let snapshot = model.runtimeStatus?.snapshot else { return "尚无数据" }
        return "revision \(snapshot.revision)"
    }

    private var snapshotFreshness: String {
        guard let snapshot = model.runtimeStatus?.snapshot else {
            return "source unavailable"
        }
        return "\(snapshot.freshness) · \(snapshot.revisionSource)"
    }

    private var storageSchema: String {
        guard let storage = model.runtimeStatus?.storage else { return "尚无数据" }
        guard let schema = storage.schemaVersion else { return storage.status }
        return "schema \(schema)"
    }

    private var storageIntegrity: String {
        guard let storage = model.runtimeStatus?.storage else {
            return "integrity unavailable"
        }
        return "integrity \(storage.integrity ?? "unavailable") · \(storage.eventCount) events"
    }

    private var layers: [DiagnosticLayerPresentation] {
        [runtimeLayer, providerLayer, tokenLayer, projectionLayer]
    }

    private var runtimeLayer: DiagnosticLayerPresentation {
        let online = model.bridgeStatus.isListening
            && model.runtimeStatus?.api.status == "ready"
        let hook = model.runtimeStatus?.hook.status ?? "unavailable"
        return DiagnosticLayerPresentation(
            id: "runtime",
            title: "Runtime 与 Hook",
            status: online ? "正常" : "需要处理",
            detail: online
                ? localizedFormat(
                    "本地 API、WebSocket 与控制连接可用；Hook socket：%@。",
                    locale: locale,
                    hook
                )
                : localized(
                    "控制连接不可用；先检查进程、runtime.lock 与 bridge.sock。",
                    locale: locale
                ),
            metadata: "source: runtime/status + local supervisor",
            recovery: online ? "无需操作" : "重启 Runtime",
            symbol: "server.rack",
            tone: online ? .good : .bad
        )
    }

    private var providerLayer: DiagnosticLayerPresentation {
        let providers = model.setupInfo?.providers ?? []
        let connected = providers.filter { $0.status == "connected" }.count
        let warning = connected < 2 || providers.contains {
            !["connected", "provider_missing"].contains($0.status)
        }
        return DiagnosticLayerPresentation(
            id: "provider",
            title: "Provider 与 Connector",
            status: connected > 0 ? (warning ? "部分可用" : "正常") : "未连接",
            detail: localizedFormat(
                "%lld/2 个 Provider 已由真实事件验证；Codex 直接能力只看 Runtime Connector 声明。",
                locale: locale,
                Int64(connected)
            ),
            metadata: "source: setup + provider-capabilities.json",
            recovery: "刷新接入状态",
            symbol: "link",
            tone: connected > 0 ? (warning ? .warning : .good) : .neutral
        )
    }

    private var reviewLayer: DiagnosticLayerPresentation {
        guard let review = model.runtimeStatus?.collectors?.review else {
            return unavailableLayer(
                id: "review",
                title: "Review 与 Git",
                symbol: "checkmark.seal"
            )
        }
        let good = review.status == "ready"
        return DiagnosticLayerPresentation(
            id: "review",
            title: "Review 与 Git",
            status: collectorStatus(review.status),
            detail: localizedFormat(
                "Baseline collector：%@；Git 状态只在打开对应任务 Review 时按需检查。",
                locale: locale,
                review.status
            ),
            metadata: "source: \(review.source) · pending \(review.pendingBaselines ?? 0)",
            recovery: good ? "任务内刷新 Review" : "重新检查",
            symbol: "checkmark.seal",
            tone: good ? .good : .warning
        )
    }

    private var tokenLayer: DiagnosticLayerPresentation {
        guard let token = model.runtimeStatus?.collectors?.token else {
            return unavailableLayer(
                id: "token",
                title: "Token Collector",
                symbol: "chart.xyaxis.line"
            )
        }
        let good = token.status == "ready"
        let warning = ["partial", "scanning", "pending"].contains(token.status)
        return DiagnosticLayerPresentation(
            id: "token",
            title: "Token Collector",
            status: collectorStatus(token.status),
            detail: localizedFormat(
                "本机账本：%@；历史扫描：%@。",
                locale: locale,
                token.dataQuality ?? "unavailable",
                token.historyComplete == true
                    ? localized("完整", locale: locale)
                    : localized("未完成", locale: locale)
            ),
            metadata: "source: \(token.source) · failures \(token.consecutiveFailures)",
            recovery: good ? "自动增量更新" : "刷新 Snapshot",
            symbol: "chart.xyaxis.line",
            tone: good ? .good : (warning ? .warning : .bad)
        )
    }

    private var companionLayer: DiagnosticLayerPresentation {
        guard let companion = model.runtimeStatus?.companion else {
            return unavailableLayer(
                id: "companion",
                title: "Companion",
                symbol: "display.2"
            )
        }
        let good = companion.status == "ready"
        let scopes = companion.scopes.isEmpty
            ? localized("无已配对客户端", locale: locale)
            : companion.scopes.joined(separator: ", ")
        return DiagnosticLayerPresentation(
            id: "companion",
            title: "Companion",
            status: good ? "正常" : "不可用",
            detail: localizedFormat(
                "协议 v%lld · %lld 个客户端 · %@",
                locale: locale,
                Int64(companion.protocolVersion),
                Int64(companion.registrations),
                scopes
            ),
            metadata: "source: runtime:companion-auth (token hash only)",
            recovery: "在设置中配对或撤销",
            symbol: "display.2",
            tone: good ? .good : .bad
        )
    }

    private var projectionLayer: DiagnosticLayerPresentation {
        guard let status = model.runtimeStatus, let snapshot = status.snapshot else {
            return unavailableLayer(
                id: "projection",
                title: "投影与 UI",
                symbol: "rectangle.3.group"
            )
        }
        let active = status.sessions.active ?? 0
        let staleWhileActive = active > 0
            && ["stale", "invalid"].contains(snapshot.freshness)
        let sync = model.lastSyncAt.map(ZhFormat.syncClock) ?? "—"
        return DiagnosticLayerPresentation(
            id: "projection",
            title: "投影与 UI",
            status: staleWhileActive ? "已过期" : "正常",
            detail: localizedFormat(
                "Snapshot %lld · %@；Native 最近同步 %@。",
                locale: locale,
                Int64(snapshot.revision),
                snapshot.freshness,
                sync
            ),
            metadata: "source: websocket snapshot + native lastSyncAt",
            recovery: staleWhileActive ? "重新检查连接" : "无需操作",
            symbol: "rectangle.3.group",
            tone: staleWhileActive ? .bad : .good
        )
    }

    private func unavailableLayer(
        id: String,
        title: String,
        symbol: String
    ) -> DiagnosticLayerPresentation {
        DiagnosticLayerPresentation(
            id: id,
            title: title,
            status: "尚无数据",
            detail: localized("Runtime 尚未返回这一层的诊断事实。", locale: locale),
            metadata: "source: unavailable",
            recovery: "重新检查",
            symbol: symbol,
            tone: .neutral
        )
    }

    private func providerTone(_ status: String?) -> DiagnosticTone {
        switch status {
        case "connected": .good
        case "installed_unverified", "needs_trust", "needs_reinstall": .warning
        case "error", "inline_conflict": .bad
        default: .neutral
        }
    }

    private func providerStatus(_ status: String?) -> String {
        switch status {
        case "connected": "已验证"
        case "installed_unverified": "等待事件"
        case "needs_trust": "需要信任"
        case "needs_reinstall": "需要修复"
        case "provider_missing", "not_installed": "未启用"
        case "inline_conflict", "error": "异常"
        case let value?: value
        case nil: "尚无数据"
        }
    }

    private func providerVersion(_ provider: String) -> String {
        guard let detail = model.runtimeDoctorReport?.check("\(provider).cli")?.detail else {
            return localized("版本尚未检查", locale: locale)
        }
        let pieces = detail.components(separatedBy: " · ")
        return pieces.last ?? detail
    }

    private func providerCapabilitySummary(_ provider: String) -> String {
        if model.isDemo {
            return localizedFormat(
                "能力合同：%lld/6 已验证%@",
                locale: locale,
                Int64(provider == "claude" ? 6 : 4),
                provider == "codex" ? " · connector connected" : ""
            )
        }
        let kind = ProviderKind(record: provider) ?? .custom(provider)
        let supported = ProviderCapabilityFeature.allCases.filter {
            model.client.snapshot.providerCapability(for: kind, feature: $0)
                .status.canClaimSupport
        }.count
        let connector = provider == "codex"
            ? model.client.snapshot.capabilities?.codexConnector?.status
            : nil
        let connectorText = connector.map { " · connector \($0)" } ?? ""
        return localizedFormat(
            "能力合同：%lld/6 已验证%@",
            locale: locale,
            Int64(supported),
            connectorText
        )
    }

    private func providerSource(_ provider: String) -> String {
        if model.isDemo {
            return provider == "claude"
                ? "source: hook:PermissionRequest"
                : "source: connector:turn/plan/updated"
        }
        let kind = ProviderKind(record: provider) ?? .custom(provider)
        let sources = ProviderCapabilityFeature.allCases.compactMap {
            model.client.snapshot.providerCapability(for: kind, feature: $0).source
        }
        return "source: \(sources.first ?? "capability_unconfirmed")"
    }

    private func collectorStatus(_ status: String) -> String {
        switch status {
        case "ready": "正常"
        case "scanning": "扫描中"
        case "partial": "部分可用"
        case "degraded": "需要处理"
        case "unavailable": "不可用"
        default: "等待首次检查"
        }
    }

    private func recoveryAvailable(
        for layer: DiagnosticLayerPresentation
    ) -> Bool {
        switch layer.id {
        case "runtime": !model.bridgeStatus.isListening
        case "provider": true
        case "review", "token", "projection": layer.status != "正常"
        default: false
        }
    }

    private func runRecovery(for layerID: String) {
        switch layerID {
        case "runtime":
            model.restartRuntime()
        case "provider":
            Task {
                await model.refreshControlPlaneDiagnostics(includeDoctor: true)
            }
        case "review", "token", "projection":
            Task {
                await model.client.refreshSnapshot()
                await model.refreshControlPlaneDiagnostics(includeDoctor: false)
            }
        default:
            break
        }
    }
}

private struct DiagnosticLayerPresentation: Identifiable {
    let id: String
    let title: String
    let status: String
    let detail: String
    let metadata: String
    let recovery: String
    let symbol: String
    let tone: DiagnosticTone
}

private enum DiagnosticTone {
    case good
    case warning
    case bad
    case neutral

    var color: Color {
        switch self {
        case .good: DT.greenDot
        case .warning: DT.amberDot
        case .bad: DT.redText
        case .neutral: DT.textWeak
        }
    }
}
