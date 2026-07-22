import AppKit
import Testing
@testable import ActRealmUI

@Suite struct StableVisualEffectTests {
    @Test @MainActor func visualEffectAlwaysUsesActiveAppearance() {
        let view = AlwaysActiveVisualEffectView.makeView(
            material: .underWindowBackground,
            blendingMode: .withinWindow
        )

        #expect(view.material == .underWindowBackground)
        #expect(view.blendingMode == .withinWindow)
        #expect(view.state == .active)
        #expect(!view.isEmphasized)
    }

    @Test @MainActor func updateRestoresActiveAppearance() {
        let view = NSVisualEffectView()
        view.state = .inactive

        AlwaysActiveVisualEffectView.configure(
            view,
            material: .contentBackground,
            blendingMode: .behindWindow
        )

        #expect(view.material == .contentBackground)
        #expect(view.blendingMode == .behindWindow)
        #expect(view.state == .active)
    }
}
