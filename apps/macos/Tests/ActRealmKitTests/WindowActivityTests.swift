import Foundation
import Testing
@testable import ActRealmKit
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

    @Test func workspaceClockUsesFiveSecondCadenceWithoutAttention() {
        let start = Date(timeIntervalSince1970: 1_000)
        #expect(!WorkspaceClockPolicy.shouldPublish(
            lastPublishedAt: start,
            now: start.addingTimeInterval(4.9),
            needsSecondPrecision: false,
            hasActiveWork: true
        ))
        #expect(WorkspaceClockPolicy.shouldPublish(
            lastPublishedAt: start,
            now: start.addingTimeInterval(5),
            needsSecondPrecision: false,
            hasActiveWork: true
        ))
        #expect(!WorkspaceClockPolicy.shouldPublish(
            lastPublishedAt: start,
            now: start.addingTimeInterval(29.9),
            needsSecondPrecision: false,
            hasActiveWork: false
        ))
        #expect(WorkspaceClockPolicy.shouldPublish(
            lastPublishedAt: start,
            now: start.addingTimeInterval(30),
            needsSecondPrecision: false,
            hasActiveWork: false
        ))
        #expect(WorkspaceClockPolicy.shouldPublish(
            lastPublishedAt: start,
            now: start.addingTimeInterval(1),
            needsSecondPrecision: true,
            hasActiveWork: false
        ))
    }
}
