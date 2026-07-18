import AppKit
import Combine
import ActRealmKit

/// Executes the AppKit half of 台前调度: foregrounding a matching Agent app,
/// observing active Space changes, and returning the ActRealm Workspace window after an
/// unaccepted dispatch. ActRealmKit remains responsible for policy/timers.
@MainActor
public final class ForegroundSchedulingController {
    private let model: AppModel
    private var cancellables: Set<AnyCancellable> = []
    private var lastHandledPhase: ForegroundDispatchPhase?
    private var lastHandledID: String?

    public init(model: AppModel) {
        self.model = model

        model.$foregroundDispatch
            .receive(on: RunLoop.main)
            .sink { [weak self] dispatch in
                self?.handle(dispatch)
            }
            .store(in: &cancellables)

        NSWorkspace.shared.notificationCenter.publisher(for: NSWorkspace.activeSpaceDidChangeNotification)
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                self?.refreshWorkspaceStatus()
                self?.acceptIfSchedulingWorkspaceIsActive()
            }
            .store(in: &cancellables)

        for name in [NSWorkspace.didLaunchApplicationNotification, NSWorkspace.didTerminateApplicationNotification] {
            NSWorkspace.shared.notificationCenter.publisher(for: name)
                .receive(on: RunLoop.main)
                .sink { [weak self] _ in self?.refreshWorkspaceStatus() }
                .store(in: &cancellables)
        }

        Task { @MainActor [weak self] in
            await Task.yield()
            self?.refreshWorkspaceStatus()
        }
    }

    private func handle(_ dispatch: ForegroundDispatchState?) {
        guard let dispatch else {
            lastHandledID = nil
            lastHandledPhase = nil
            return
        }
        guard dispatch.id != lastHandledID || dispatch.phase != lastHandledPhase else { return }
        lastHandledID = dispatch.id
        lastHandledPhase = dispatch.phase

        switch dispatch.phase {
        case .reminding:
            break
        case .opening:
            activateAgent(for: dispatch.provider)
        case .awaitingWorkspace:
            // App activation and Mission Control notifications settle on the
            // following run-loop turn. If the Agent was already beside Act
            // Room, this immediately satisfies the documented receive rule.
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(180))
                self?.acceptIfSchedulingWorkspaceIsActive()
            }
        case .returnedToActRealmWorkspace:
            activateActRealmWorkspace()
        }
    }

    private func activateAgent(for provider: ProviderKind?) {
        guard let target = agentTarget(for: provider) else {
            model.showToast("未找到对应 Agent 窗口；任务仍保留在 ActRealm 工作区")
            refreshWorkspaceStatus()
            return
        }
        _ = target.activate(options: [])
        refreshWorkspaceStatus()
    }

    private func agentTarget(for provider: ProviderKind?) -> NSRunningApplication? {
        let running = NSWorkspace.shared.runningApplications.filter { !$0.isTerminated }
        let providerTokens: [String]
        switch provider {
        case .codex: providerTokens = ["codex"]
        case .claude: providerTokens = ["claude"]
        case .gemini: providerTokens = ["gemini"]
        case nil: providerTokens = ["codex", "claude", "gemini", "cursor"]
        }

        return running.first(where: { app in
            let identity = "\(app.bundleIdentifier ?? "") \(app.localizedName ?? "")".lowercased()
            return providerTokens.contains { identity.contains($0) }
        }) ?? running.first(where: { app in
            let identity = "\(app.bundleIdentifier ?? "") \(app.localizedName ?? "")".lowercased()
            return ["terminal", "iterm", "warp", "wezterm", "alacritty"].contains {
                identity.contains($0)
            }
        })
    }

    private func acceptIfSchedulingWorkspaceIsActive() {
        guard model.foregroundDispatch?.phase == .awaitingWorkspace,
              let window = actRealmWorkspaceWindow,
              window.isOnActiveSpace
        else { return }
        model.acceptForegroundWorkspace()
    }

    private func activateActRealmWorkspace() {
        NSApplication.shared.activate(ignoringOtherApps: true)
        actRealmWorkspaceWindow?.makeKeyAndOrderFront(nil)
        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(120))
            self?.refreshWorkspaceStatus()
        }
    }

    private func refreshWorkspaceStatus() {
        let window = actRealmWorkspaceWindow
        model.updateForegroundWorkspaceStatus(ForegroundWorkspaceStatus(
            isActRealmWorkspaceReady: window != nil,
            isAgentAvailable: agentTarget(for: model.foregroundDispatch?.provider) != nil,
            isSchedulingWorkspaceActive: window?.isOnActiveSpace ?? false
        ))
    }

    private var actRealmWorkspaceWindow: NSWindow? {
        NSApplication.shared.windows.first {
            !($0 is NSPanel) && $0.isVisible && $0.canBecomeMain
        }
    }
}
