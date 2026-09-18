/// Product-scope switches for the current local control plane.
///
/// Build 59 surface capabilities are enabled unless a newer subsystem is kept
/// intentionally parked or requires a separately authorized release slice.
public enum ProductScope {
    public static let reviewEnabled = false
    public static let metadataCheckpointEnabled = false
    public static let advancedTokenAnalyticsEnabled = true
    public static let developerDisplayCustomizationEnabled = true
    public static let animatedThemeMediaEnabled = true
    public static let localUsageStatsEnabled = true
}
