import AppKit
import Testing
@testable import ActRealmUI
@testable import ActRealmKit
import SwiftUI

@Suite struct HUDPanelLayoutTests {
    @Test @MainActor func hostingViewAcceptsTheFirstClickWithoutMovingThePanel() {
        let host = HUDInteractiveHostingView(rootView: EmptyView())

        #expect(host.acceptsFirstMouse(for: nil))
        #expect(!host.mouseDownCanMoveWindow)
    }

    @Test func decisionButtonsDisappearImmediatelyAfterOneSubmission() {
        let submission = HUDDecisionSubmission(attentionID: "approval-1", action: .approve)

        #expect(HUDDecisionInteraction.showsDecisionButtons(
            entryID: "approval-1",
            state: .open,
            submission: nil
        ))
        #expect(!HUDDecisionInteraction.showsDecisionButtons(
            entryID: "approval-1",
            state: .open,
            submission: submission
        ))
        #expect(!HUDDecisionInteraction.showsDecisionButtons(
            entryID: "approval-1",
            state: .committing,
            submission: nil
        ))
    }

    @Test @MainActor func closingPreviewHidesOnlyTheHUDAndKeepsTheOutbox() {
        let model = AppModel(defaults: isolatedHUDDefaults(), demo: true)
        model.start()
        defer { model.shutdown() }
        let entries = model.derived.openOutbox

        model.previewHUD()
        #expect(model.isHUDPreviewActive)
        model.dismissHUD()

        #expect(!model.isHUDPreviewActive)
        #expect(model.derived.openOutbox == entries)
    }

    @Test func centersCapsuleAtTopOfPrimaryVisibleArea() {
        let origin = HUDPanelLayout.origin(
            screenFrame: NSRect(x: 0, y: 0, width: 1_512, height: 982),
            visibleFrame: NSRect(x: 0, y: 0, width: 1_512, height: 958),
            safeAreaInsets: .init(top: 0, left: 0, bottom: 0, right: 0),
            panelSize: NSSize(width: 560, height: 90)
        )

        #expect(origin.x == 476)
        #expect(origin.y == 860)
    }

    @Test func notchSafeAreaMovesCapsuleBelowTopObstruction() {
        let origin = HUDPanelLayout.origin(
            screenFrame: NSRect(x: 0, y: 0, width: 1_512, height: 982),
            visibleFrame: NSRect(x: 0, y: 0, width: 1_512, height: 982),
            safeAreaInsets: .init(top: 74, left: 0, bottom: 0, right: 0),
            panelSize: NSSize(width: 600, height: 90)
        )

        #expect(origin.x == 456)
        #expect(origin.y == 810)
        #expect(origin.y + 90 <= 982 - 74)
    }

    @Test func globalCoordinatesCenterCapsuleOnOffsetDisplay() {
        let origin = HUDPanelLayout.origin(
            screenFrame: NSRect(x: -1_920, y: 120, width: 1_920, height: 1_080),
            visibleFrame: NSRect(x: -1_920, y: 120, width: 1_920, height: 1_056),
            safeAreaInsets: .init(top: 0, left: 0, bottom: 0, right: 0),
            panelSize: NSSize(width: 640, height: 96)
        )

        #expect(origin.x == -1_280)
        #expect(origin.y == 1_072)
    }

    @Test func selectedDisplayFallsBackByNameAfterReconnect() {
        let displays = [
            HUDDisplayOption(id: 1, name: "Built-in Display", isMain: true),
            HUDDisplayOption(id: 9, name: "Studio Display", isMain: false),
        ]

        let resolved = HUDDisplaySelection.resolve(
            mode: .selectedDisplay,
            selectedID: 7,
            selectedName: "Studio Display",
            mainID: 1,
            actRealmWindowID: nil,
            available: displays
        )

        #expect(resolved == 9)
    }

    @Test func unavailableSelectedDisplayTemporarilyUsesMainDisplay() {
        let displays = [
            HUDDisplayOption(id: 1, name: "Built-in Display", isMain: true)
        ]

        let resolved = HUDDisplaySelection.resolve(
            mode: .selectedDisplay,
            selectedID: 7,
            selectedName: "Studio Display",
            mainID: 1,
            actRealmWindowID: nil,
            available: displays
        )

        #expect(resolved == 1)
    }

    @Test func automaticModeUsesActRealmWindowDisplay() {
        let displays = [
            HUDDisplayOption(id: 1, name: "Built-in Display", isMain: true),
            HUDDisplayOption(id: 2, name: "External Display", isMain: false),
        ]

        let resolved = HUDDisplaySelection.resolve(
            mode: .followActRealmWindow,
            selectedID: nil,
            selectedName: nil,
            mainID: 1,
            actRealmWindowID: 2,
            available: displays
        )

        #expect(resolved == 2)
    }
}

private func isolatedHUDDefaults() -> UserDefaults {
    let suite = "HUDPanelLayoutTests.\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: suite)!
    defaults.removePersistentDomain(forName: suite)
    return defaults
}
