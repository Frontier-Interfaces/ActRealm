import AppKit
import ActRealmKit
import SwiftUI

/// 330pt glass card inside the MenuBarExtra window.
public struct MenuBarPopoverView: View {
    public init() {}

    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

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
                Text("暂无需要处理的事项")
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
            Text("ActRealm")
                .font(.system(size: 13, weight: .bold))
                .foregroundStyle(DT.textStrong)
            Spacer()
            listeningPill
        }
    }

    private var listeningPill: some View {
        let tone = runtimeTone
        return HStack(spacing: 6) {
            StatusDot(
                color: tone.dotColor,
                size: 6
            )
            Text(pillText)
                .font(.system(size: 10.5, weight: .semibold))
                .foregroundStyle(tone.textColor)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 3)
        .background(Capsule().fill(tone.backgroundColor))
        .overlay(Capsule().strokeBorder(tone.strokeColor, lineWidth: 1))
    }

    private var pillText: String {
        switch model.bridgeStatus {
        case .listening: "本机在线"
        case .starting: "启动中…"
        case .absent: "Runtime 未连接"
        }
    }

    private var runtimeTone: MenuBarStatusTone {
        switch model.bridgeStatus {
        case .listening: .green
        case .starting: .blue
        case .absent: .red
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
        let presentation = MenuBarLanePresentation(lane: lane, now: model.now)
        return HStack(spacing: 9) {
            ProviderAvatar(kind: lane.provider, size: 22)
            VStack(alignment: .leading, spacing: 0) {
                Text(presentation.title)
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(Color(lightWhite: 0.85, darkWhite: 0.88))
                    .lineLimit(1)
                Text(presentation.subtitle)
                    .font(DT.body(10.5))
                    .foregroundStyle(DT.textWeak)
                    .lineLimit(1)
            }
            Spacer()
            Text(presentation.trailing)
                .font(.system(size: 10.5, weight: presentation.tone == .neutral ? .regular : .semibold))
                .foregroundStyle(presentation.tone.textColor)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 7)
        .contentShape(Rectangle())
        .hoverBrightens()
        .onTapGesture { openMainWindow() }
    }

    // MARK: Footer

    private var footer: some View {
        HStack {
            Button("打开 ActRealm") { openMainWindow() }
            Spacer()
            Button("设置…") { openSettingsWindow() }
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
        NSApp.activate(ignoringOtherApps: true)
        if let window = NSApp.windows.first(where: {
            guard let id = $0.identifier?.rawValue else { return false }
            return id == "main" || id.hasPrefix("main-AppWindow-")
        }) {
            window.deminiaturize(nil)
            window.makeKeyAndOrderFront(nil)
        } else {
            openWindow(id: "main")
        }
    }

    private func openSettingsWindow() {
        NSApp.activate(ignoringOtherApps: true)
        openWindow(id: "settings")
    }
}

// MARK: - Menu bar presentation

enum MenuBarStatusTone: Equatable {
    case amber
    case red
    case blue
    case green
    case neutral

    init(kind: OutboxKind) {
        switch kind {
        case .approval, .nativeApproval: self = .amber
        case .question: self = .blue
        case .error: self = .red
        case .completion: self = .green
        }
    }

    var textColor: Color {
        switch self {
        case .amber: DT.amberText
        case .red: DT.redText
        case .blue: DT.blueText
        case .green: DT.greenText
        case .neutral: DT.textFaint
        }
    }

    var dotColor: Color {
        switch self {
        case .amber: DT.amberDot
        case .red: DT.redRing
        case .blue: DT.blue
        case .green: DT.greenDot
        case .neutral: DT.textFaint
        }
    }

    var backgroundColor: Color {
        switch self {
        case .amber: DT.amberBg
        case .red: DT.redBg
        case .blue: DT.blueBg
        case .green: DT.greenBg
        case .neutral: DT.cardSoft
        }
    }

    var strokeColor: Color {
        switch self {
        case .amber: DT.amberStroke
        case .red: DT.redStroke
        case .blue: DT.blueBadgeStroke
        case .green: DT.greenStroke
        case .neutral: DT.hairlineSoft
        }
    }
}

struct MenuBarLanePresentation: Equatable {
    let title: String
    let subtitle: String
    let trailing: String
    let tone: MenuBarStatusTone

    init(lane: Lane, now: Date) {
        let waiting = lane.tasks.filter { $0.status == .waiting }
        let failed = lane.tasks.filter { $0.status == .failed }
        let running = lane.tasks.filter { $0.status == .running }
        let featured = waiting.first ?? failed.first ?? running.first ?? lane.tasks.first

        title = featured?.projectName ?? featured?.title ?? lane.provider.displayName

        if lane.provider == .gemini {
            subtitle = "\(lane.provider.displayName) · 仅通知"
        } else if let featured {
            switch featured.status {
            case .waiting: subtitle = "\(lane.provider.displayName) · 等待处理"
            case .running: subtitle = "\(lane.provider.displayName) · \(featured.activity ?? "正在运行")"
            case .failed: subtitle = "\(lane.provider.displayName) · 运行失败"
            case .done: subtitle = "\(lane.provider.displayName) · 本轮已完成"
            case .idle: subtitle = "\(lane.provider.displayName) · 空闲"
            }
        } else {
            subtitle = "\(lane.provider.displayName) · 无活动任务"
        }

        if !waiting.isEmpty {
            trailing = "\(waiting.count) 项待处理"
            tone = .amber
        } else if !failed.isEmpty {
            trailing = "\(failed.count) 项出错"
            tone = .red
        } else if !running.isEmpty {
            trailing = "\(running.count) 项运行中"
            tone = .blue
        } else if let featured {
            trailing = ZhFormat.shortAge(now.timeIntervalSince(featured.lastEventAt))
            tone = .neutral
        } else {
            trailing = "无活动"
            tone = .neutral
        }
    }
}

// MARK: - Compact approval card

private struct CompactApprovalCard: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow
    @State private var confirmingAllow = false
    let entry: OutboxEntry

    private var tone: MenuBarStatusTone { MenuBarStatusTone(kind: entry.kind) }

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 7) {
                StatusDot(color: tone.dotColor, size: 7)
                Text("\(entry.provider?.displayName ?? entry.attention.provider) · \(entry.kind.badgeText)")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(tone.textColor)
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
        .sheetCard(fill: tone.backgroundColor, stroke: tone.strokeColor, radius: DT.radiusCard)
    }

    private var contextLine: String {
        var parts: [String] = []
        if let project = entry.attention.project { parts.append(project) }
        if let provider = entry.provider {
            switch provider {
            case .claude: parts.append("24 小时未回复将交回 Provider")
            case .codex: parts.append("1 小时未回复将交回 Provider")
            case .gemini: parts.append("仅通知")
            case .custom: parts.append("由连接器定义")
            }
        }
        return parts.joined(separator: " · ")
    }

    @ViewBuilder
    private var buttons: some View {
        switch entry.kind {
        case .approval where entry.risk.needsVerification:
            Button("拒绝") { model.deny(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("去核对") { model.passThrough(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .approval:
            Button("拒绝") { model.deny(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("允许") { model.approve(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .nativeApproval:
            Button("打开应用") { Task { await model.jump(to: entry) } }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .question:
            Button("在 ActRealm 回答") {
                model.revealSession(for: entry)
                openWindow(id: "main")
                NSApplication.shared.activate(ignoringOtherApps: true)
            }
            .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .error:
            Button("稍后提醒") { model.snooze(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("标记已处理") { model.acknowledge(entry) }
                .buttonStyle(PillButtonStyle(rank: .primary, fontSize: 11.5, horizontalPadding: 14))
        case .completion:
            Button("稍后提醒") { model.snooze(entry) }
                .buttonStyle(PillButtonStyle(rank: .secondary, fontSize: 11.5, horizontalPadding: 14))
            Button("确认完成") { model.acknowledge(entry) }
                .buttonStyle(ActionButtonStyle(kind: .success, compact: true))
        }
    }
}

// MARK: - Menu bar label (icon)

public struct MenuBarLabel: View {
    public init() {}

    @EnvironmentObject var model: AppModel

    public var body: some View {
        let absent = !model.bridgeStatus.isListening
        HStack(spacing: 3) {
            Image(nsImage: Self.barsImage)
                .opacity(absent ? 0.28 : 1)
            if let entry = model.derived.openOutbox.first, !absent {
                let tone = MenuBarStatusTone(kind: entry.kind)
                Circle()
                    .fill(tone.dotColor)
                    .frame(width: 6, height: 6)
                    .shadow(color: tone.dotColor.opacity(0.8), radius: 2)
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
