import AppKit
import ActRealmKit
import ActRealmUI
import SwiftUI

@main
struct ActRealmApp: App {
    @StateObject private var model: AppModel
    @State private var hudController: HUDPanelController?
    @State private var foregroundSchedulingController: ForegroundSchedulingController?
    @State private var tokenThresholdNotificationController:
        TokenThresholdNotificationController?

    init() {
        NativeLanguageBootstrap.apply(AppLocalization.selectedLanguage())
        let model = AppModel(repoPath: Self.devRepoPath())
        _model = StateObject(wrappedValue: model)
        NSApplication.shared.setActivationPolicy(.regular)
    }

    var body: some Scene {
        Window("", id: "main") {
            MainWindowView()
                .environmentObject(model)
                .environment(\.locale, model.interfaceLocale)
                .modifier(TokenDashboardNotificationRouter())
                .task {
                    if tokenThresholdNotificationController == nil {
                        tokenThresholdNotificationController =
                            TokenThresholdNotificationController()
                    }
                    model.start()
                    if hudController == nil {
                        hudController = HUDPanelController(model: model)
                    }
                    if foregroundSchedulingController == nil {
                        foregroundSchedulingController = ForegroundSchedulingController(model: model)
                    }
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                    model.shutdown()
                }
                .onReceive(model.$tokenThresholdNotices) { notices in
                    if model.tokenUsage.collectionState == "ready",
                       model.tokenUsage.dataQuality == "verified",
                       !model.tokenUsage.collectionInProgress {
                        tokenThresholdNotificationController?.deliver(notices)
                    }
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
                    Task { await model.refreshSetup() }
                }
                .onReceive(
                    NSWorkspace.shared.notificationCenter.publisher(
                        for: NSWorkspace.didWakeNotification
                    )
                ) { _ in
                    model.handleSystemWake()
                }
        }
        .defaultSize(width: 1440, height: 820)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentMinSize)
        .windowBackgroundDragBehavior(.enabled)
        .commands {
            CommandGroup(replacing: .newItem) {}
            SettingsWindowCommands(language: model.appLanguage)
            CommandGroup(replacing: .appTermination) {
                Button(AppLocalization.localized("退出 ActRealm", language: model.appLanguage)) {
                    NSApplication.shared.terminate(nil)
                }
                .keyboardShortcut("q", modifiers: .command)
            }
        }

        MenuBarExtra {
            MenuBarPopoverView()
                .environmentObject(model)
                .environment(\.locale, model.interfaceLocale)
        } label: {
            MenuBarLabel()
                .environmentObject(model)
                .environment(\.locale, model.interfaceLocale)
        }
        .menuBarExtraStyle(.window)

        Window(AppLocalization.localized("设置", language: model.appLanguage), id: "settings") {
            SettingsView()
                .environmentObject(model)
                .environment(\.locale, model.interfaceLocale)
                .onChange(of: model.appLanguage) { _, language in
                    NativeLanguageBootstrap.apply(language)
                    model.objectWillChange.send()
                }
        }
        .defaultSize(width: 920, height: 660)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)

        Window(AppLocalization.localized("Token 仪表板", language: model.appLanguage), id: "token-dashboard") {
            TokenUsageDashboardView()
                .environmentObject(model)
                .environment(\.locale, model.interfaceLocale)
        }
        .defaultSize(width: 1180, height: 760)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentMinSize)
        .windowBackgroundDragBehavior(.enabled)
    }

    /// Dev fallback: the monorepo's Runtime workspace. Packaged builds use the
    /// helper embedded in ActRealm.app instead.
    private static func devRepoPath() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // ActRealmApp/
            .deletingLastPathComponent() // Sources/
            .deletingLastPathComponent() // macos/
            .deletingLastPathComponent() // apps/
    }
}

private struct TokenDashboardNotificationRouter: ViewModifier {
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        content.onReceive(NotificationCenter.default.publisher(
            for: .actRealmOpenTokenDashboard
        )) { _ in
            openWindow(id: "token-dashboard")
        }
    }
}

private struct SettingsWindowCommands: Commands {
    let language: AppLanguage
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .appSettings) {
            Button(AppLocalization.localized("设置…", language: language)) { openWindow(id: "settings") }
                .keyboardShortcut(",", modifiers: .command)
        }
    }
}

/// Use the saved app language for AppKit's standard menus on launch, without
/// changing macOS language preferences or persisting an AppleLanguages override.
@MainActor
private enum NativeLanguageBootstrap {
    private static let originalArguments = UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)

    static func apply(_ language: AppLanguage) {
        var arguments = originalArguments
        if language != .system { arguments["AppleLanguages"] = [language.rawValue] }
        UserDefaults.standard.setVolatileDomain(arguments, forName: UserDefaults.argumentDomain)
    }
}
