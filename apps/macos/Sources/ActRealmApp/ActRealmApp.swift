import AppKit
import ActRealmKit
import ActRealmUI
import SwiftUI

@main
struct ActRealmApp: App {
    @StateObject private var model: AppModel
    @State private var hudController: HUDPanelController?
    @State private var foregroundSchedulingController: ForegroundSchedulingController?

    init() {
        let model = AppModel(repoPath: Self.devRepoPath())
        _model = StateObject(wrappedValue: model)
        NSApplication.shared.setActivationPolicy(.regular)
    }

    var body: some Scene {
        Window("", id: "main") {
            MainWindowView()
                .environmentObject(model)
                .task {
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
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
                    Task { await model.refreshSetup() }
                }
        }
        .defaultSize(width: 1440, height: 820)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentMinSize)
        .windowBackgroundDragBehavior(.enabled)
        .commands {
            CommandGroup(replacing: .newItem) {}
            SettingsWindowCommands()
            CommandGroup(replacing: .appTermination) {
                Button("退出 ActRealm") {
                    NSApplication.shared.terminate(nil)
                }
                .keyboardShortcut("q", modifiers: .command)
            }
        }

        MenuBarExtra {
            MenuBarPopoverView()
                .environmentObject(model)
        } label: {
            MenuBarLabel()
                .environmentObject(model)
        }
        .menuBarExtraStyle(.window)

        Window("设置", id: "settings") {
            SettingsView()
                .environmentObject(model)
        }
        .defaultSize(width: 920, height: 660)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
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

private struct SettingsWindowCommands: Commands {
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .appSettings) {
            Button("设置…") { openWindow(id: "settings") }
                .keyboardShortcut(",", modifiers: .command)
        }
    }
}
