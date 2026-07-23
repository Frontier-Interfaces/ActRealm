import CoreGraphics
import Testing
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
}
