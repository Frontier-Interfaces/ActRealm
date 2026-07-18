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
        WindowGroup("", id: "main") {
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
        }
        .defaultSize(width: 1440, height: 820)
        .windowResizability(.contentMinSize)
        .windowBackgroundDragBehavior(.enabled)
        .commands {
            CommandGroup(replacing: .newItem) {}
        }

        MenuBarExtra {
            MenuBarPopoverView()
                .environmentObject(model)
        } label: {
            MenuBarLabel()
                .environmentObject(model)
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView()
                .environmentObject(model)
        }
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
