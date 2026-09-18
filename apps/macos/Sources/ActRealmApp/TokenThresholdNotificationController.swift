import ActRealmKit
import Foundation
@preconcurrency import UserNotifications

@MainActor
final class TokenThresholdNotificationController: NSObject, UNUserNotificationCenterDelegate {
    private static let deliveredKey = "actrealm.tokenThresholdNotifications.delivered"
    private let center: UNUserNotificationCenter
    private let defaults: UserDefaults

    init(
        center: UNUserNotificationCenter = .current(),
        defaults: UserDefaults = .standard
    ) {
        self.center = center
        self.defaults = defaults
        super.init()
        center.delegate = self
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        guard response.notification.request.content.userInfo["route"] as? String == "token_dashboard" else { return }
        await MainActor.run {
            NotificationCenter.default.post(name: .actRealmOpenTokenDashboard, object: nil)
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }

    func deliver(_ rates: [TokenUsageBurnRate]) {
        guard !rates.isEmpty else { return }
        var delivered = defaults.dictionary(forKey: Self.deliveredKey)
            as? [String: Double] ?? [:]
        let now = Date().timeIntervalSince1970
        delivered = delivered.filter { now - $0.value < 7 * 24 * 60 * 60 }
        let pending = rates.filter { rate in
            let key = notificationKey(rate)
            guard delivered[key] == nil else { return false }
            delivered[key] = now
            return true
        }
        defaults.set(delivered, forKey: Self.deliveredKey)
        for rate in pending {
            Task { [weak self] in await self?.schedule(rate) }
        }
    }

    private func notificationKey(_ rate: TokenUsageBurnRate) -> String {
        rate.turnId
    }

    private func schedule(_ rate: TokenUsageBurnRate) async {
        let settings = await center.notificationSettings()
        let authorized: Bool
        switch settings.authorizationStatus {
        case .authorized, .provisional:
            authorized = true
        case .notDetermined:
            authorized = (try? await center.requestAuthorization(
                options: [.alert, .sound]
            )) == true
        case .denied:
            authorized = false
        @unknown default:
            authorized = false
        }
        guard authorized else { return }

        let content = UNMutableNotificationContent()
        content.title = AppLocalization.localized(
            "Token 燃烧速度达到本机阈值",
            defaults: defaults
        )
        let label = rate.title ?? rate.project ?? rate.provider
        content.body = AppLocalization.formatted(
            "%@：%lld Token / 分钟。打开 ActRealm 查看真实窗口与基线。",
            label,
            Int64(clamping: rate.tokensPerMinute),
            defaults: defaults
        )
        content.sound = .default
        content.categoryIdentifier = "actrealm.token.threshold"
        content.userInfo = [
            "route": "token_dashboard",
            "session_id": rate.sessionId,
            "turn_id": rate.turnId,
        ]
        let request = UNNotificationRequest(
            identifier: "actrealm-token-threshold-\(rate.turnId)",
            content: content,
            trigger: nil
        )
        try? await center.add(request)
    }
}

extension Notification.Name {
    static let actRealmOpenTokenDashboard = Notification.Name("com.frontierinterfaces.actrealm.open-token-dashboard")
}
