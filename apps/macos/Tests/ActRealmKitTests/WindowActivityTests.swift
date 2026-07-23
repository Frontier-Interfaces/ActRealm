import Testing
@testable import ActRealmUI

@Suite struct WindowActivityTests {
    @Test func aVisibleNonKeyWindowKeepsLiveRenderingEnabled() {
        #expect(WindowRenderPolicy.shouldRender(
            isVisible: true,
            isMiniaturized: false,
            isOcclusionVisible: true
        ))
    }

    @Test func minimizedOrOccludedWindowsPauseRendering() {
        #expect(!WindowRenderPolicy.shouldRender(
            isVisible: true,
            isMiniaturized: true,
            isOcclusionVisible: true
        ))
        #expect(!WindowRenderPolicy.shouldRender(
            isVisible: true,
            isMiniaturized: false,
            isOcclusionVisible: false
        ))
    }
}
