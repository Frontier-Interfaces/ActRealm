import AppKit
import ActRealmKit
import SwiftUI

/// 330pt glass card inside the MenuBarExtra window.
public struct MenuBarPopoverView: View {
    public init() {}

    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    private var entries: [OutboxEntry] { model.derived.openOutbox }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if let first = entries.first {
                sectionLabel("OUTBOX · \(entries.count)")
                CompactApprovalCard(entry: first)
                ForEach(entries.dropFirst().prefix(2)) { entry in
                    queueRow(entry)
                        .padding(.top, 6)
                }
            } else {
                sectionLabel("OUTBOX")
                Text("现在没有需要你处理的任务")
                    .font(DT.body(11))
                    .foregroundStyle(DT.textWeak)
                    .padding(.vertical, 8)
            }
            sectionLabel("LANES")
            VStack(spacing: 0) {
                ForEach(model.derived.lanes) { lane in
                    laneRow(lane)
                }
            }
            footer
        }
        .padding(14)
        .frame(width: 330)
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 8) {
            LogoMark()
            Text("flow-agent")
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(DT.textStrong)
            Spacer()
            listeningPill
        }
    }

    private var listeningPill: some View {
        HStack(spacing: 6) {
            StatusDot(
                color: model.bridgeStatus.isListening ? DT.greenDot : Color(lightWhite: 0.3, darkWhite: 0.4),
                size: 6
            )
            Text(pillText)
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(model.bridgeStatus.isListening ? DT.greenText : DT.textWeak)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 3)
        .background(Capsule().fill(model.bridgeStatus.isListening ? DT.greenBg : DT.neutralBadgeBg))
        .overlay(Capsule().strokeBorder(
            model.bridgeStatus.isListening ? DT.greenStroke : DT.neutralBadgeStroke, lineWidth: 1))
    }

    private var pillText: String {
        switch model.bridgeStatus {
        case .listening: "Listening"
        case .starting: "启动中…"
        case .absent: "Runtime absent · fail-open"
        }
    }

    // MARK: Sections

    private func sectionLabel(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 10, weight: .bold))
            .kerning(0.8)
            .foregroundStyle(Color(lightWhite: 0.4, darkWhite: 0.4))
            .padding(.top, 12)
            .padding(.bottom, 6)
    }

    private func queueRow(_ entry: OutboxEntry) -> some View {
        HStack(spacing: 8) {
            Chip(text: entry.kind.badgeText, tone: .forOutboxKind(entry.kind), fontSize: 9)
            Text(entry.actionTitle)
                .font(.system(size: 11.5, weight: .semibold))
                .foregroundStyle(Color(lightWhite: 0.8, darkWhite: 0.88))
                .lineLimit(1)
            Spacer(minLength: 4)
            Text(ZhFormat.shortAge(model.now.timeIntervalSince(entry.createdAt)))
                .font(DT.micro(10))
                .foregroundStyle(DT.textFaint)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .sheetCard(fill: DT.cardSoft, stroke: DT.hairlineSoft, radius: 10)
    }

    private func laneRow(_ lane: Lane) -> some View {
        HStack(spacing: 9) {
            ProviderAvatar(kind: lane.provider, size: 22)
            VStack(alignment: .leading, spacing: 0) {
                Text(laneTitle(lane))
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Color(lightWhite: 0.85, darkWhite: 0.88))
                    .lineLimit(1)
                Text(laneSubtitle(lane))
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
            }
            Spacer()
            trailing(lane)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 7)
        .contentShape(Rectangle())
        .hoverBrightens()
        .onTapGesture { openMainWindow() }
    }

    private func laneTitle(_ lane: Lane) -> String {
        lane.tasks.first?.projectName ?? lane.tasks.first?.title ?? lane.provider.displayName
    }

    private func laneSubtitle(_ lane: Lane) -> String {
        if lane.provider == .gemini { return "gemini · notify-only" }
        if let top = lane.tasks.first {
            switch top.status {
            case .waiting: return "\(lane.provider.displayName) · 等待你处理"
            case .running: return "\(lane.provider.displayName) · \(top.activity ?? "在跑")"
            case .failed: return "\(lane.provider.displayName) · 运行失败"
            case .done: return "\(lane.provider.displayName) · 本轮已完成"
            case .idle: return "\(lane.provider.displayName) · 空闲"
            }
        }
        return "\(lane.provider.displayName) · 无活动任务"
    }

    @ViewBuilder
    private func trailing(_ lane: Lane) -> some View {
        if lane.waitingCount > 0 {
            Text("\(lane.waitingCount) waiting")
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(DT.amberText)
        } else if let top = lane.tasks.first {
            Text(ZhFormat.shortAge(model.now.timeIntervalSince(top.lastEventAt)))
                .font(DT.micro(10.5))
                .foregroundStyle(DT.textFaint)
        } else {
            Text("idle")
                .font(DT.micro(10.5))
                .foregroundStyle(DT.textFaint)
        }
    }

    // MARK: Footer

    private var footer: some View {
        HStack {
            Button("Open flow-agent…") { openMainWindow() }
            Spacer()
            Button("Settings…") {
                NSApp.activate(ignoringOtherApps: true)
                openSettings()
            }
        }
        .buttonStyle(.plain)
        .font(.system(size: 11.5, weight: .semibold))
        .foregroundStyle(Color(lightWhite: 0.6, darkWhite: 0.65))
        .padding(.top, 10)
        .overlay(alignment: .top) {
            Rectangle().fill(DT.separator).frame(height: 1)
        }
        .padding(.top, 6)
    }

    private func openMainWindow() {
        openWindow(id: "main")
        NSApp.activate(ignoringOtherApps: true)
    }
}

// MARK: - Compact approval card

private struct CompactApprovalCard: View {
    @EnvironmentObject var model: AppModel
    @State private var confirmingAllow = false
    let entry: OutboxEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 7) {
                StatusDot(color: entry.provider.map { DT.providerText($0).opacity(0.85) } ?? DT.amberDot, size: 7)
                Text("\(entry.provider?.displayName ?? entry.attention.provider) · \(kindLabel)")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(DT.amberText)
                Spacer()
                Text("已等 \(ZhFormat.waitDuration(model.now.timeIntervalSince(entry.createdAt)))")
                    .font(DT.micro(9.5))
                    .foregroundStyle(DT.textWeak)
            }
            if entry.kind == .approval {
                HStack(spacing: 6) {
                    Text("\(entry.toolName ?? "操作") — \(entry.attention.commandPreview ?? "")")
                        .font(DT.mono(12))
                        .foregroundStyle(DT.textPrimary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(
                    RoundedRectangle(cornerRadius: 8, style: .continuous).fill(DT.commandBoxBg)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .strokeBorder(DT.commandBoxStroke, lineWidth: 1)
                )
            } else if let detail = entry.attention.detail {
                Text(detail)
                    .font(DT.body(11))
                    .foregroundStyle(DT.textSecondary)
                    .lineLimit(2)
            }
            Text(contextLine)
                .font(DT.micro(10.5))
                .foregroundStyle(DT.textWeak)
            HStack(spacing: 8) {
                Spacer()
                buttons
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 11)
        .sheetCard(fill: DT.amberBg, stroke: DT.amberStroke, radius: DT.radiusCard)
    }

    private var kindLabel: String {
        switch entry.kind {
        case .approval: "PermissionRequest"
        case .question: "提问"
        case .error: "运行出错"
        case .completion: "完成确认"
        }
    }

    private var contextLine: String {
        var parts: [String] = []
        if let project = entry.attention.project { parts.append(project) }
        if let provider = entry.provider {
            switch provider {
            case .claude: parts.append("no reply in 24h → fails open")
            case .codex: parts.append("no reply in 1h → fails open")
            case .gemini: parts.append("notify-only")
            }
        }
        return parts.joined(separator: " · ")
    }

    @ViewBuilder
    private var buttons: some View {
        switch entry.kind {
        case .approval where entry.risk.needsVerification:
            Button("Deny") { model.deny(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("去核对") { model.passThrough(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .approval:
            Button("Deny") { model.deny(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("Allow") { model.approve(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .question, .error:
            Button("稍后提醒") { model.snooze(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("标记已处理") { model.acknowledge(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .completion:
            Button("稍后提醒") { model.snooze(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("确认完成") { model.acknowledge(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        }
    }
}

// MARK: - Menu bar label (icon)

public struct MenuBarLabel: View {
    public init() {}

    @EnvironmentObject var model: AppModel

    public var body: some View {
        let waiting = !model.derived.openOutbox.isEmpty
        let absent = !model.bridgeStatus.isListening
        HStack(spacing: 3) {
            Image(nsImage: Self.barsImage)
                .opacity(absent ? 0.28 : 1)
            if waiting && !absent {
                Circle()
                    .fill(Color(nsColor: .rgba(255, 159, 10, 1)))
                    .frame(width: 6, height: 6)
                    .shadow(color: Color(nsColor: .rgba(255, 159, 10, 0.8)), radius: 2)
            }
        }
    }

    /// Three rounded bars as a template image so the menu bar tints it.
    static let barsImage: NSImage = {
        let size = NSSize(width: 15, height: 13)
        let image = NSImage(size: size, flipped: false) { rect in
            let heights: [CGFloat] = [6, 11, 8]
            var x: CGFloat = 1
            for height in heights {
                let bar = NSBezierPath(
                    roundedRect: NSRect(x: x, y: 1, width: 3, height: height),
                    xRadius: 1.5, yRadius: 1.5
                )
                NSColor.black.setFill()
                bar.fill()
                x += 5
            }
            return true
        }
        image.isTemplate = true
        return image
    }()
}
