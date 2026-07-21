import Foundation
import Testing
@testable import ActRealmKit

@Suite struct HUDSettingsTests {
    @Test func legacySettingsDefaultToSystemMainDisplay() throws {
        let legacy = Data(#"{"isEnabled":true,"displaySeconds":12,"fields":["event"]}"#.utf8)

        let settings = try JSONDecoder().decode(HUDSettings.self, from: legacy)

        #expect(settings.displayMode == .systemMain)
        #expect(settings.selectedDisplayID == nil)
        #expect(settings.selectedDisplayName == nil)
        #expect(settings.displaySeconds == 12)
        #expect(settings.fields == [.event])
    }

    @Test func unknownFutureDisplayModeSafelyFallsBackToSystemMain() throws {
        let encoded = Data(#"{"displayMode":"futureMode"}"#.utf8)

        let settings = try JSONDecoder().decode(HUDSettings.self, from: encoded)

        #expect(settings.displayMode == .systemMain)
    }
}
