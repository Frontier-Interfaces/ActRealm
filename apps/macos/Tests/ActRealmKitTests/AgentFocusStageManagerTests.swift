import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct AgentFocusStageManagerTests {
    @Test func boundWorkspacePresenceUsesTheWholeDesktopNotOnlyTheDispatchProvider() {
        let boundApplications = [
            ForegroundWorkspaceApp(
                bundleIdentifier: "com.openai.chat",
                name: "ChatGPT"
            ),
            ForegroundWorkspaceApp(
                bundleIdentifier: "com.google.Chrome",
                name: "Google Chrome"
            ),
        ]

        #expect(ForegroundWorkspacePresence.isActive(
            boundApplications: boundApplications,
            visibleApplications: [
                ForegroundWorkspaceApp(
                    bundleIdentifier: "COM.GOOGLE.CHROME",
                    name: "Google Chrome"
                ),
            ]
        ))
    }

    @Test func boundWorkspacePresenceRejectsAnotherDesktop() {
        #expect(!ForegroundWorkspacePresence.isActive(
            boundApplications: [
                ForegroundWorkspaceApp(
                    bundleIdentifier: "com.openai.chat",
                    name: "ChatGPT"
                ),
            ],
            visibleApplications: [
                ForegroundWorkspaceApp(
                    bundleIdentifier: "com.apple.TextEdit",
                    name: "TextEdit"
                ),
            ]
        ))
    }

    @Test func systemControllerAppliesPreferenceWithoutRestartingWindowManager() {
        final class State {
            var enabled = false
            var calls: [(String, [String])] = []
        }

        let state = State()
        let controller = SystemStageManagerController { executable, arguments in
            state.calls.append((executable, arguments))
            switch arguments.first {
            case "write":
                state.enabled = arguments.last == "true"
                return (0, "")
            case "read":
                return (0, state.enabled ? "1\n" : "0\n")
            default:
                return (1, "")
            }
        }

        #expect(controller.setEnabled(true))
        #expect(state.calls.count == 2)
        #expect(state.calls.allSatisfy { $0.0 == "/usr/bin/defaults" })
        #expect(state.calls[0].1 == [
            "write", "com.apple.WindowManager", "GloballyEnabled", "-bool", "true",
        ])
        #expect(state.calls[1].1 == [
            "read", "com.apple.WindowManager", "GloballyEnabled",
        ])
    }

    @Test func neverOwnsOrRestoresUserEnabledStageManager() {
        var lease = StageManagerLease()

        lease.begin(
            allowed: true,
            restoreTiming: .onReturnToActRealm,
            originalState: true,
            enableSucceeded: true
        )

        #expect(!lease.didEnableStageManager)
        #expect(!lease.shouldRestore(for: .actRealmReturn))
    }

    @Test func restoresOnlyAtConfiguredBoundary() {
        var lease = StageManagerLease()

        lease.begin(
            allowed: true,
            restoreTiming: .onReturnToActRealm,
            originalState: false,
            enableSucceeded: true
        )

        #expect(lease.didEnableStageManager)
        #expect(!lease.shouldRestore(for: .acceptance))
        #expect(lease.shouldRestore(for: .actRealmReturn))
        lease.finishRestore(succeeded: true)
        #expect(!lease.didEnableStageManager)
    }

    @Test func acceptancePolicyRestoresOnPointerAcceptance() {
        var lease = StageManagerLease()

        lease.begin(
            allowed: true,
            restoreTiming: .afterAcceptance,
            originalState: false,
            enableSucceeded: true
        )

        #expect(lease.shouldRestore(for: .acceptance))
        #expect(!lease.shouldRestore(for: .actRealmReturn))
    }

    @Test func keepEnabledNeverRequestsAReset() {
        var lease = StageManagerLease()

        lease.begin(
            allowed: true,
            restoreTiming: .keepEnabled,
            originalState: false,
            enableSucceeded: true
        )

        #expect(!lease.shouldRestore(for: .acceptance))
        #expect(!lease.shouldRestore(for: .actRealmReturn))
        #expect(!lease.shouldRestore(for: .focusDisabled))
    }

    @Test func failedEnableDoesNotCreateRestoreAuthority() {
        var lease = StageManagerLease()

        lease.begin(
            allowed: true,
            restoreTiming: .afterAcceptance,
            originalState: false,
            enableSucceeded: false
        )

        #expect(!lease.didEnableStageManager)
        #expect(!lease.shouldRestore(for: .focusDisabled))
    }
}
