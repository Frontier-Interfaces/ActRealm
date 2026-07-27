import CoreGraphics
import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite struct MainWindowLayoutTests {
    @Test func quotaColumnKeepsItsMinimumAndEveryColumnGrowsOnWideWindows() {
        let minimum = WorkspaceColumnLayout.resolve(containerWidth: 1128)
        #expect(minimum.quota == WorkspaceColumnLayout.minimumQuotaWidth)
        #expect(minimum.tasks >= 480)

        let wide = WorkspaceColumnLayout.resolve(containerWidth: 1968)
        #expect(wide.quota > minimum.quota)
        #expect(wide.outbox > minimum.outbox)
        #expect(wide.tasks > minimum.tasks)

        let expected = 1968 - WorkspaceColumnLayout.gap * 2
        #expect(abs(wide.outbox + wide.tasks + wide.quota - expected) < 0.001)
    }

    @Test func englishQuotaColumnGetsRoomForLongerUsageLabels() {
        let english = WorkspaceColumnLayout.resolve(
            containerWidth: 1128,
            language: .english
        )
        #expect(english.quota == WorkspaceColumnLayout.minimumEnglishQuotaWidth)
        #expect(english.quota > WorkspaceColumnLayout.minimumQuotaWidth)
        #expect(english.tasks >= 420)

        let expected = 1128 - WorkspaceColumnLayout.gap * 2
        #expect(abs(english.outbox + english.tasks + english.quota - expected) < 0.001)
    }

    @Test func englishMenuBarPopoverGetsRoomForLongerLabels() {
        #expect(MenuBarPopoverLayout.width(language: .simplifiedChinese) == 330)
        #expect(MenuBarPopoverLayout.width(language: .english) == 360)
    }

    @Test @MainActor func demoQuotaModeChangeKeepsSnapshotContent() {
        let suite = "ActRealmMainWindowLayoutTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        let model = AppModel(defaults: defaults, demo: true)
        model.start()
        let quotaCount = model.derived.quotaSlots.count
        #expect(quotaCount > 0)

        model.updateUISettings { $0.quotaDisplayMode = .compact }
        #expect(model.uiSettings.quotaDisplayMode == .compact)
        #expect(model.derived.quotaSlots.count == quotaCount)
    }
}
