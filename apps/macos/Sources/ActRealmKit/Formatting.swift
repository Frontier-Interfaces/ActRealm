import Foundation

/// Chinese-language formatting helpers shared by the main window, HUD, and
/// menu bar popover. All functions are pure so they can be unit tested.
public enum ZhFormat {
    /// "6 分 02 秒" / "48 秒" / "1 小时 04 分"
    public static func waitDuration(_ interval: TimeInterval) -> String {
        let total = max(0, Int(interval))
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let seconds = total % 60
        if hours > 0 {
            return String(format: "%d 小时 %02d 分", hours, minutes)
        }
        if minutes > 0 {
            return String(format: "%d 分 %02d 秒", minutes, seconds)
        }
        return "\(seconds) 秒"
    }

    /// Compact age for queue rows: "刚刚" / "2 分" / "1 小时" / "3 天"
    public static func shortAge(_ interval: TimeInterval) -> String {
        let total = max(0, Int(interval))
        if total < 60 { return "刚刚" }
        if total < 3600 { return "\(total / 60) 分" }
        if total < 86_400 { return "\(total / 3600) 小时" }
        return "\(total / 86_400) 天"
    }

    /// "12 分钟前" / "刚刚" / "3 小时前"
    public static func relativeAgo(_ interval: TimeInterval) -> String {
        let total = max(0, Int(interval))
        if total < 60 { return "刚刚" }
        if total < 3600 { return "\(total / 60) 分钟前" }
        if total < 86_400 { return "\(total / 3600) 小时前" }
        return "\(total / 86_400) 天前"
    }

    /// "54 分钟后过期" / "23 小时后过期" / "已过期"
    public static func expiry(_ interval: TimeInterval) -> String {
        if interval <= 0 { return "已过期" }
        let total = Int(interval)
        if total < 3600 { return "\(max(1, total / 60)) 分钟后过期" }
        if total < 86_400 { return "\(total / 3600) 小时后过期" }
        return "\(total / 86_400) 天后过期"
    }

    /// "周一 00:00 重置" if on another day, "14:30 重置" if today.
    public static func resetTime(_ resetsAt: Date, now: Date, calendar: Calendar = .current) -> String {
        let formatter = DateFormatter()
        formatter.calendar = calendar
        formatter.dateFormat = "HH:mm"
        let clock = formatter.string(from: resetsAt)
        if calendar.isDate(resetsAt, inSameDayAs: now) {
            return "\(clock) 重置"
        }
        let weekdayNames = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"]
        let weekday = calendar.component(.weekday, from: resetsAt) - 1
        let name = weekdayNames[max(0, min(6, weekday))]
        return "\(name) \(clock) 重置"
    }

    /// "09:41:22" for the bottom status bar.
    public static func syncClock(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        return formatter.string(from: date)
    }

    /// Compact provider usage without pretending that rounded values are
    /// exact: 48329 -> "48.3K", 1250000 -> "1.25M".
    public static func tokenCount(_ count: UInt64) -> String {
        switch count {
        case 1_000_000...:
            let value = Double(count) / 1_000_000
            return value >= 10 ? String(format: "%.1fM", value) : String(format: "%.2fM", value)
        case 1_000...:
            let value = Double(count) / 1_000
            return value >= 100 ? String(format: "%.0fK", value) : String(format: "%.1fK", value)
        default:
            return "\(count)"
        }
    }

    /// Milliseconds-since-epoch helper used across snapshot records.
    public static func date(fromMillis millis: UInt64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(millis) / 1000)
    }
}
