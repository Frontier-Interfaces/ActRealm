import ActRealmKit
import AppKit
import UniformTypeIdentifiers
import Charts
import SwiftUI

enum TokenDashboardPeriod: String, CaseIterable, Identifiable {
    case day
    case month
    case total

    var id: String { rawValue }

    func value(in totals: TokenUsageTotals) -> UInt64 {
        switch self {
        case .day: totals.today
        case .month: totals.month
        case .total: totals.total
        }
    }

    func value(in provider: TokenUsageProviderTotal) -> UInt64 {
        switch self {
        case .day: provider.today
        case .month: provider.month
        case .total: provider.total
        }
    }

    func observedTime(in totals: TokenUsageTotals) -> UInt64 {
        switch self {
        case .day: totals.todayActiveTimeSeconds
        case .month: totals.monthActiveTimeSeconds
        case .total: totals.activeTimeSeconds
        }
    }

    func executionTime(in totals: TokenUsageTotals) -> UInt64 {
        switch self {
        case .day: totals.todayExecutionTimeSeconds
        case .month: totals.monthExecutionTimeSeconds
        case .total: totals.executionTimeSeconds
        }
    }
}

enum TokenDashboardPage: String, CaseIterable, Identifiable {
    case overview
    case trends

    var id: String { rawValue }
}

enum TokenDashboardBreakdown: String, CaseIterable, Identifiable {
    case provider
    case model

    var id: String { rawValue }
}

enum TokenDashboardRange: String, CaseIterable, Identifiable {
    case seven
    case thirty
    case ninety
    case year
    case all

    var id: String { rawValue }

    var dayCount: Int? {
        switch self {
        case .seven: 7
        case .thirty: 30
        case .ninety: 90
        case .year: 365
        case .all: nil
        }
    }
}

enum TokenDashboardHeatmapMetric: String, CaseIterable, Identifiable {
    case tokens
    case cost

    var id: String { rawValue }
}

struct TokenDashboardBreakdownRow: Identifiable, Equatable {
    let id: String
    let label: String
    let provider: String
    let total: UInt64
    let inputTokens: UInt64?
    let outputTokens: UInt64?
    let cacheReadTokens: UInt64?
    let cacheCreationTokens: UInt64?
    let reasoningTokens: UInt64?
    let estimatedCostUsdMicros: UInt64?
    let pricedTokens: UInt64
    let unpricedTokens: UInt64

    init(
        id: String,
        label: String,
        provider: String,
        total: UInt64,
        inputTokens: UInt64?,
        outputTokens: UInt64?,
        cacheReadTokens: UInt64?,
        cacheCreationTokens: UInt64?,
        reasoningTokens: UInt64?,
        estimatedCostUsdMicros: UInt64?,
        pricedTokens: UInt64? = nil,
        unpricedTokens: UInt64? = nil
    ) {
        self.id = id
        self.label = label
        self.provider = provider
        self.total = total
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cacheReadTokens = cacheReadTokens
        self.cacheCreationTokens = cacheCreationTokens
        self.reasoningTokens = reasoningTokens
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricedTokens = pricedTokens ?? (estimatedCostUsdMicros == nil ? 0 : total)
        self.unpricedTokens = unpricedTokens ?? (estimatedCostUsdMicros == nil ? total : 0)
    }
}

struct TokenDashboardComposition: Equatable {
    let uncachedInputTokens: UInt64
    let cacheReadTokens: UInt64
    let cacheCreationTokens: UInt64
    let outputTokens: UInt64
    let reasoningTokens: UInt64
    let unclassifiedTokens: UInt64
    let hasComponentData: Bool
    let hasInconsistentTotals: Bool

    var classifiedTotal: UInt64 {
        uncachedInputTokens
            .saturatingAdd(cacheReadTokens)
            .saturatingAdd(cacheCreationTokens)
            .saturatingAdd(outputTokens)
    }

    var displayTotal: UInt64 {
        classifiedTotal.saturatingAdd(unclassifiedTokens)
    }

    var cacheHitRate: Double? {
        let eligible = uncachedInputTokens.saturatingAdd(cacheReadTokens)
        guard eligible > 0 else { return nil }
        return Double(cacheReadTokens) / Double(eligible)
    }
}

private struct TokenDashboardComponentPart: Identifiable {
    let id: String
    let label: String
    let value: UInt64
    let color: Color
}

struct TokenDashboardDay: Identifiable, Equatable {
    let day: String
    let date: Date
    let total: UInt64
    let estimatedCostUsdMicros: UInt64?
    let pricedTokens: UInt64
    let unpricedTokens: UInt64
    let messageCount: UInt64
    let byProvider: [TokenUsageDayProviderTotal]
    let byModel: [TokenUsageDayModelTotal]

    var id: String { day }

    init(
        day: String,
        date: Date,
        total: UInt64,
        estimatedCostUsdMicros: UInt64?,
        pricedTokens: UInt64? = nil,
        unpricedTokens: UInt64? = nil,
        messageCount: UInt64,
        byProvider: [TokenUsageDayProviderTotal],
        byModel: [TokenUsageDayModelTotal]
    ) {
        self.day = day
        self.date = date
        self.total = total
        self.estimatedCostUsdMicros = estimatedCostUsdMicros
        self.pricedTokens = pricedTokens ?? (estimatedCostUsdMicros == nil ? 0 : total)
        self.unpricedTokens = unpricedTokens ?? (estimatedCostUsdMicros == nil ? total : 0)
        self.messageCount = messageCount
        self.byProvider = byProvider
        self.byModel = byModel
    }
}

struct TokenDashboardSeriesPoint: Identifiable, Equatable {
    let day: String
    let date: Date
    let series: String
    let value: UInt64

    var id: String { "\(day):\(series)" }
}

struct TokenDashboardSeriesSummary: Identifiable, Equatable {
    let series: String
    let total: UInt64
    let share: Double

    var id: String { series }
}

struct TokenDashboardStat: Identifiable, Equatable {
    let label: String
    let value: String
    let detail: String?

    var id: String { label }
}

struct TokenDashboardHeatmapCell: Identifiable, Equatable {
    let day: String
    let date: Date
    let column: Int
    let weekday: Int
    let tokens: UInt64
    let costUsdMicros: UInt64?
    let pricedTokens: UInt64
    let unpricedTokens: UInt64
    let tokenLevel: Int
    let costLevel: Int
    let included: Bool
    let hasRecord: Bool

    var id: String { day }

    func level(for metric: TokenDashboardHeatmapMetric) -> Int {
        metric == .tokens ? tokenLevel : costLevel
    }
}

struct TokenDashboardPricingSummary: Equatable {
    let estimatedCostUsdMicros: UInt64?
    let pricedTokens: UInt64
    let unpricedTokens: UInt64

    var totalTokens: UInt64 { pricedTokens.saturatingAdd(unpricedTokens) }
    var coverage: Double? {
        guard totalTokens > 0 else { return nil }
        return Double(pricedTokens) / Double(totalTokens)
    }
    var isPartial: Bool { pricedTokens > 0 && unpricedTokens > 0 }
}

struct TokenDashboardMonthLabel: Identifiable, Equatable {
    let date: Date
    let column: Int

    var id: Date { date }
}

struct TokenDashboardHeatmapCalendar: Equatable {
    let cells: [TokenDashboardHeatmapCell]
    let monthLabels: [TokenDashboardMonthLabel]
    let weeks: Int
}

enum TokenDashboardPresentation {
    static func tokenText(_ value: UInt64, unitStyle: TokenUsageUnitStyle, locale: Locale) -> String {
        let usesEastAsianUnits = unitStyle == .eastAsian
            || (unitStyle == .automatic
                && locale.identifier.lowercased().hasPrefix("zh"))
        let number = Double(value)
        if usesEastAsianUnits {
            if value >= 100_000_000 {
                let scaled = number / 100_000_000
                return scaled >= 10
                    ? String(format: "%.1f亿", scaled)
                    : String(format: "%.2f亿", scaled)
            }
            if value >= 10_000 {
                let scaled = number / 10_000
                return scaled >= 100
                    ? String(format: "%.0f万", scaled)
                    : String(format: "%.1f万", scaled)
            }
            let formatter = NumberFormatter()
            formatter.locale = locale
            formatter.numberStyle = .decimal
            return formatter.string(from: NSNumber(value: value)) ?? String(value)
        }
        if value >= 1_000_000_000 {
            return String(format: "%.2fB", number / 1_000_000_000)
        }
        if value >= 1_000_000 {
            return String(format: "%.2fM", number / 1_000_000)
        }
        if value >= 1_000 {
            return String(format: "%.1fK", number / 1_000)
        }
        let formatter = NumberFormatter()
        formatter.locale = locale
        formatter.numberStyle = .decimal
        return formatter.string(from: NSNumber(value: value)) ?? String(value)
    }


    static func hasObservedData(_ totals: TokenUsageTotals) -> Bool {
        totals.total > 0 || totals.today > 0 || totals.month > 0
            || !totals.byProvider.isEmpty || !totals.recentDays.isEmpty
            || totals.collectionState == "ready"
    }

    static func analyticsAreFinal(_ totals: TokenUsageTotals) -> Bool {
        totals.dataQuality == "verified"
            && totals.collectionState == "ready"
            && !totals.collectionInProgress
    }

    static func providerRows(
        totals: TokenUsageTotals,
        period: TokenDashboardPeriod,
        now: Date = Date(),
        calendar: Calendar = .current
    ) -> [TokenDashboardBreakdownRow] {
        if period == .total {
            return totals.byProvider.compactMap { provider in
                let value = period.value(in: provider)
                guard value > 0 || provider.total == 0 else { return nil }
                let models = totals.byModel.filter { $0.provider == provider.provider }
                return TokenDashboardBreakdownRow(
                    id: "provider:\(provider.provider)",
                    label: providerDisplayName(provider.provider),
                    provider: provider.provider,
                    total: value,
                    inputTokens: optionalSum(models.map(\.inputTokens)),
                    outputTokens: optionalSum(models.map(\.outputTokens)),
                    cacheReadTokens: optionalSum(models.map(\.cacheReadTokens)),
                    cacheCreationTokens: optionalSum(models.map(\.cacheCreationTokens)),
                    reasoningTokens: optionalSum(models.map(\.reasoningTokens)),
                    estimatedCostUsdMicros: optionalSum(
                        models.map(\.estimatedCostUsdMicros)
                    ),
                    pricedTokens: models.reduce(UInt64(0)) {
                        $0.saturatingAdd(modelPricedTokens($1))
                    },
                    unpricedTokens: models.reduce(UInt64(0)) {
                        $0.saturatingAdd(modelUnpricedTokens($1))
                    }
                )
            }.sorted(by: breakdownSort)
        }

        let days = periodDays(
            totals: totals,
            period: period,
            now: now,
            calendar: calendar
        )
        var values: [String: TokenDashboardBreakdownRow] = [:]
        for day in days {
            let modelsByProvider = Dictionary(grouping: day.byModel, by: \.provider)
            for provider in day.byProvider {
                let id = "provider:\(provider.provider)"
                let previous = values[id]
                let models = modelsByProvider[provider.provider] ?? []
                values[id] = TokenDashboardBreakdownRow(
                    id: id,
                    label: providerDisplayName(provider.provider),
                    provider: provider.provider,
                    total: (previous?.total ?? 0).saturatingAdd(provider.total),
                    inputTokens: optionalAdd(
                        previous?.inputTokens,
                        optionalSum(models.map(\.inputTokens))
                    ),
                    outputTokens: optionalAdd(
                        previous?.outputTokens,
                        optionalSum(models.map(\.outputTokens))
                    ),
                    cacheReadTokens: optionalAdd(
                        previous?.cacheReadTokens,
                        optionalSum(models.map(\.cacheReadTokens))
                    ),
                    cacheCreationTokens: optionalAdd(
                        previous?.cacheCreationTokens,
                        optionalSum(models.map(\.cacheCreationTokens))
                    ),
                    reasoningTokens: optionalAdd(
                        previous?.reasoningTokens,
                        optionalSum(models.map(\.reasoningTokens))
                    ),
                    estimatedCostUsdMicros: optionalAdd(
                        previous?.estimatedCostUsdMicros,
                        provider.estimatedCostUsdMicros
                    ),
                    pricedTokens: (previous?.pricedTokens ?? 0)
                        .saturatingAdd(provider.pricedTokens),
                    unpricedTokens: (previous?.unpricedTokens ?? 0)
                        .saturatingAdd(provider.unpricedTokens)
                )
            }
        }
        return values.values.sorted(by: breakdownSort)
    }

    static func modelRows(
        totals: TokenUsageTotals,
        period: TokenDashboardPeriod,
        now: Date,
        calendar: Calendar
    ) -> [TokenDashboardBreakdownRow] {
        if period == .total {
            return totals.byModel.map { model in
                TokenDashboardBreakdownRow(
                    id: "model:\(model.provider):\(model.model)",
                    label: model.model,
                    provider: model.provider,
                    total: model.total,
                    inputTokens: model.inputTokens,
                    outputTokens: model.outputTokens,
                    cacheReadTokens: model.cacheReadTokens,
                    cacheCreationTokens: model.cacheCreationTokens,
                    reasoningTokens: model.reasoningTokens,
                    estimatedCostUsdMicros: model.estimatedCostUsdMicros,
                    pricedTokens: modelPricedTokens(model),
                    unpricedTokens: modelUnpricedTokens(model)
                )
            }.sorted(by: breakdownSort)
        }

        let days = periodDays(
            totals: totals,
            period: period,
            now: now,
            calendar: calendar
        )
        var values: [String: TokenDashboardBreakdownRow] = [:]
        for day in days {
            for model in day.byModel {
                let id = "model:\(model.provider):\(model.model)"
                let previous = values[id]
                values[id] = TokenDashboardBreakdownRow(
                    id: id,
                    label: model.model,
                    provider: model.provider,
                    total: (previous?.total ?? 0).saturatingAdd(model.total),
                    inputTokens: optionalAdd(previous?.inputTokens, model.inputTokens),
                    outputTokens: optionalAdd(previous?.outputTokens, model.outputTokens),
                    cacheReadTokens: optionalAdd(
                        previous?.cacheReadTokens,
                        model.cacheReadTokens
                    ),
                    cacheCreationTokens: optionalAdd(
                        previous?.cacheCreationTokens,
                        model.cacheCreationTokens
                    ),
                    reasoningTokens: optionalAdd(
                        previous?.reasoningTokens,
                        model.reasoningTokens
                    ),
                    estimatedCostUsdMicros: optionalAdd(
                        previous?.estimatedCostUsdMicros,
                        model.estimatedCostUsdMicros
                    ),
                    pricedTokens: (previous?.pricedTokens ?? 0)
                        .saturatingAdd(model.pricedTokens),
                    unpricedTokens: (previous?.unpricedTokens ?? 0)
                        .saturatingAdd(model.unpricedTokens)
                )
            }
        }
        return values.values.sorted(by: breakdownSort)
    }

    /// Normalizes Provider-specific token semantics into non-overlapping
    /// components. Codex reports cached input as a subset of input_tokens,
    /// while Claude reports cache read/create alongside uncached input.
    static func composition(
        rows: [TokenDashboardBreakdownRow]
    ) -> TokenDashboardComposition {
        var uncachedInput: UInt64 = 0
        var cacheRead: UInt64 = 0
        var cacheCreation: UInt64 = 0
        var output: UInt64 = 0
        var reasoning: UInt64 = 0
        var unclassified: UInt64 = 0
        var hasComponentData = false
        var hasInconsistentTotals = false

        for row in rows {
            let rawInput = row.inputTokens ?? 0
            let rowCacheRead = row.cacheReadTokens ?? 0
            let rowCacheCreation = row.cacheCreationTokens ?? 0
            let rowOutput = row.outputTokens ?? 0
            let rowReasoning = row.reasoningTokens ?? 0
            hasComponentData = hasComponentData
                || row.inputTokens != nil
                || row.outputTokens != nil
                || row.cacheReadTokens != nil
                || row.cacheCreationTokens != nil
                || row.reasoningTokens != nil

            let rowUncachedInput: UInt64
            if row.provider.caseInsensitiveCompare("codex") == .orderedSame {
                rowUncachedInput = rawInput >= rowCacheRead
                    ? rawInput - rowCacheRead
                    : 0
                hasInconsistentTotals = hasInconsistentTotals || rowCacheRead > rawInput
            } else {
                rowUncachedInput = rawInput
            }

            let classified = rowUncachedInput
                .saturatingAdd(rowCacheRead)
                .saturatingAdd(rowCacheCreation)
                .saturatingAdd(rowOutput)
            if classified <= row.total {
                unclassified = unclassified.saturatingAdd(row.total - classified)
            } else {
                hasInconsistentTotals = true
            }
            uncachedInput = uncachedInput.saturatingAdd(rowUncachedInput)
            cacheRead = cacheRead.saturatingAdd(rowCacheRead)
            cacheCreation = cacheCreation.saturatingAdd(rowCacheCreation)
            output = output.saturatingAdd(rowOutput)
            reasoning = reasoning.saturatingAdd(rowReasoning)
        }

        return TokenDashboardComposition(
            uncachedInputTokens: uncachedInput,
            cacheReadTokens: cacheRead,
            cacheCreationTokens: cacheCreation,
            outputTokens: output,
            reasoningTokens: reasoning,
            unclassifiedTokens: unclassified,
            hasComponentData: hasComponentData,
            hasInconsistentTotals: hasInconsistentTotals
        )
    }

    static func periodDays(
        totals: TokenUsageTotals,
        period: TokenDashboardPeriod,
        now: Date,
        calendar: Calendar
    ) -> [TokenUsageDayTotal] {
        let today = dayKey(now, calendar: calendar)
        let month = String(today.prefix(7))
        switch period {
        case .day:
            return totals.recentDays.filter { $0.day == today }
        case .month:
            return totals.recentDays.filter { $0.day.hasPrefix(month) }
        case .total:
            return totals.recentDays
        }
    }

    static func filledDays(
        totals: TokenUsageTotals,
        count: Int,
        now: Date,
        calendar inputCalendar: Calendar
    ) -> [TokenDashboardDay] {
        var calendar = inputCalendar
        calendar.locale = Locale(identifier: "en_US_POSIX")
        let source = Dictionary(uniqueKeysWithValues: totals.recentDays.map { ($0.day, $0) })
        let start = calendar.startOfDay(for: now)
        return (0..<max(1, count)).reversed().compactMap { offset in
            guard let date = calendar.date(byAdding: .day, value: -offset, to: start) else {
                return nil
            }
            let key = dayKey(date, calendar: calendar)
            return dashboardDay(date: date, key: key, source: source[key])
        }
    }

    static func rangeDays(
        totals: TokenUsageTotals,
        range: TokenDashboardRange,
        now: Date,
        calendar: Calendar
    ) -> [TokenDashboardDay] {
        if let count = range.dayCount {
            return filledDays(
                totals: totals,
                count: count,
                now: now,
                calendar: calendar
            )
        }
        let source = Dictionary(uniqueKeysWithValues: totals.recentDays.map { ($0.day, $0) })
        guard
            let firstKey = totals.recentDays.map(\.day).min(),
            let firstDate = date(from: firstKey, calendar: calendar)
        else {
            return []
        }
        let end = calendar.startOfDay(for: now)
        let count = max(
            1,
            (calendar.dateComponents([.day], from: firstDate, to: end).day ?? 0) + 1
        )
        return (0..<count).compactMap { offset in
            guard let date = calendar.date(byAdding: .day, value: offset, to: firstDate) else {
                return nil
            }
            let key = dayKey(date, calendar: calendar)
            return dashboardDay(date: date, key: key, source: source[key])
        }
    }

    static func activeRangeDays(
        totals: TokenUsageTotals,
        range: TokenDashboardRange,
        now: Date,
        calendar: Calendar
    ) -> [TokenDashboardDay] {
        let cutoff = range.dayCount.flatMap {
            calendar.date(
                byAdding: .day,
                value: -max(0, $0 - 1),
                to: calendar.startOfDay(for: now)
            )
        }
        return totals.recentDays.compactMap { source in
            guard let date = date(from: source.day, calendar: calendar) else { return nil }
            if let cutoff, date < cutoff { return nil }
            return dashboardDay(date: date, key: source.day, source: source)
        }.sorted { $0.date < $1.date }
    }

    static func seriesPoints(
        days: [TokenDashboardDay],
        breakdown: TokenDashboardBreakdown
    ) -> [TokenDashboardSeriesPoint] {
        days.flatMap { day in
            var values: [String: UInt64] = [:]
            switch breakdown {
            case .provider:
                for provider in day.byProvider {
                    let label = providerDisplayName(provider.provider)
                    values[label] = (values[label] ?? 0).saturatingAdd(provider.total)
                }
            case .model:
                for model in day.byModel {
                    values[model.model] = (values[model.model] ?? 0)
                        .saturatingAdd(model.total)
                }
            }
            return values.compactMap { entry -> TokenDashboardSeriesPoint? in
                let series = entry.key
                let value = entry.value
                guard value > 0 else { return nil }
                return TokenDashboardSeriesPoint(
                    day: day.day,
                    date: day.date,
                    series: series,
                    value: value
                )
            }.sorted { $0.series < $1.series }
        }
    }

    static func seriesSummaries(
        points: [TokenDashboardSeriesPoint]
    ) -> [TokenDashboardSeriesSummary] {
        var totals: [String: UInt64] = [:]
        for point in points {
            totals[point.series] = (totals[point.series] ?? 0).saturatingAdd(point.value)
        }
        let grandTotal = totals.values.reduce(UInt64(0)) { $0.saturatingAdd($1) }
        return totals.map {
            TokenDashboardSeriesSummary(
                series: $0.key,
                total: $0.value,
                share: grandTotal == 0 ? 0 : Double($0.value) / Double(grandTotal)
            )
        }.filter { $0.total > 0 }.sorted {
            if $0.total != $1.total { return $0.total > $1.total }
            return $0.series < $1.series
        }
    }

    static func heatmapCalendar(
        totals: TokenUsageTotals,
        now: Date,
        calendar inputCalendar: Calendar
    ) -> TokenDashboardHeatmapCalendar {
        var calendar = inputCalendar
        calendar.locale = Locale(identifier: "en_US_POSIX")
        let end = calendar.startOfDay(for: now)
        let monthStart = calendar.date(
            from: calendar.dateComponents([.year, .month], from: end)
        ) ?? end
        let requestedStart = calendar.date(
            byAdding: .month,
            value: -11,
            to: monthStart
        ) ?? monthStart
        let weekday = calendar.component(.weekday, from: requestedStart) - 1
        let gridStart = calendar.date(
            byAdding: .day,
            value: -weekday,
            to: requestedStart
        ) ?? requestedStart
        let source = Dictionary(uniqueKeysWithValues: totals.recentDays.map { ($0.day, $0) })
        let dayCount = max(
            1,
            (calendar.dateComponents([.day], from: gridStart, to: end).day ?? 0) + 1
        )
        let raw = (0..<dayCount).compactMap { offset
            -> (String, Date, Int, Int, UInt64, UInt64?, UInt64, UInt64, Bool)? in
            guard let date = calendar.date(byAdding: .day, value: offset, to: gridStart)
            else { return nil }
            let key = dayKey(date, calendar: calendar)
            let record = source[key]
            return (
                key,
                date,
                offset / 7,
                calendar.component(.weekday, from: date) - 1,
                record?.total ?? 0,
                record.flatMap { day in
                    day.estimatedCostUsdMicros ?? (day.total == 0 ? 0 : nil)
                },
                record?.pricedTokens ?? 0,
                record?.unpricedTokens ?? 0,
                date >= requestedStart
            )
        }
        let tokenScale = heatmapScale(raw.filter(\.8).map(\.4))
        let costScale = heatmapScale(raw.filter(\.8).compactMap(\.5))
        let cells = raw.map {
            TokenDashboardHeatmapCell(
                day: $0.0,
                date: $0.1,
                column: $0.2,
                weekday: $0.3,
                tokens: $0.4,
                costUsdMicros: $0.5,
                pricedTokens: $0.6,
                unpricedTokens: $0.7,
                tokenLevel: heatmapLevel(value: $0.4, scale: tokenScale),
                costLevel: $0.5.map { heatmapLevel(value: $0, scale: costScale) } ?? 0,
                included: $0.8,
                hasRecord: source[$0.0] != nil
            )
        }
        var labels: [TokenDashboardMonthLabel] = []
        var cursor = requestedStart
        while cursor <= end {
            let column = max(
                0,
                (calendar.dateComponents([.day], from: gridStart, to: cursor).day ?? 0)
                    / 7
            )
            labels.append(TokenDashboardMonthLabel(date: cursor, column: column))
            guard let next = calendar.date(byAdding: .month, value: 1, to: cursor)
            else { break }
            cursor = next
        }
        return TokenDashboardHeatmapCalendar(
            cells: cells,
            monthLabels: labels,
            weeks: (cells.last?.column ?? -1) + 1
        )
    }

    static func heatmapTooltipDate(
        _ item: TokenDashboardHeatmapCell,
        locale: Locale,
        calendar: Calendar
    ) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.calendar = calendar
        formatter.timeZone = calendar.timeZone
        formatter.setLocalizedDateFormatFromTemplate("MMM d")
        return formatter.string(from: item.date)
    }

    static func heatmapTooltipValue(
        _ item: TokenDashboardHeatmapCell,
        metric: TokenDashboardHeatmapMetric,
        locale: Locale
    ) -> String {
        guard item.hasRecord else { return localized("该日未采到用量记录", locale: locale) }
        switch metric {
        case .tokens:
            let formatter = NumberFormatter()
            formatter.locale = locale
            formatter.numberStyle = .decimal
            let tokens = formatter.string(from: NSNumber(value: item.tokens))
                ?? String(item.tokens)
            return localizedFormat("使用了 %@ Token", locale: locale, tokens)
        case .cost:
            guard let cost = item.costUsdMicros else {
                return localized("费用未提供", locale: locale)
            }
            if item.unpricedTokens > 0 {
                return localizedFormat(
                    "至少 %@ · 定价覆盖 %@",
                    locale: locale,
                    tokenDashboardFormattedCost(cost, locale: locale),
                    coveragePercent(
                        priced: item.pricedTokens,
                        unpriced: item.unpricedTokens
                    )
                )
            }
            return localizedFormat(
                "当日 API 等价值 %@",
                locale: locale,
                tokenDashboardFormattedCost(cost, locale: locale)
            )
        }
    }

    static func selectedPricingSummary(
        totals: TokenUsageTotals,
        period: TokenDashboardPeriod,
        now: Date,
        calendar: Calendar
    ) -> TokenDashboardPricingSummary {
        let days = periodDays(
            totals: totals,
            period: period,
            now: now,
            calendar: calendar
        )
        let direct = optionalSum(days.map(\.estimatedCostUsdMicros))
        let modelFallback = optionalSum(
            days.flatMap(\.byModel).map(\.estimatedCostUsdMicros)
        )
        return TokenDashboardPricingSummary(
            estimatedCostUsdMicros: direct ?? modelFallback,
            pricedTokens: days.reduce(UInt64(0)) {
                $0.saturatingAdd($1.pricedTokens)
            },
            unpricedTokens: days.reduce(UInt64(0)) {
                $0.saturatingAdd($1.unpricedTokens)
            }
        )
    }

    static func selectedCost(
        totals: TokenUsageTotals,
        period: TokenDashboardPeriod,
        now: Date,
        calendar: Calendar
    ) -> UInt64? {
        selectedPricingSummary(
            totals: totals,
            period: period,
            now: now,
            calendar: calendar
        ).estimatedCostUsdMicros
    }

    static func dayKey(_ date: Date, calendar: Calendar) -> String {
        let components = calendar.dateComponents([.year, .month, .day], from: date)
        return String(
            format: "%04d-%02d-%02d",
            components.year ?? 1970,
            components.month ?? 1,
            components.day ?? 1
        )
    }

    static func date(from day: String, calendar: Calendar) -> Date? {
        let parts = day.split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        return calendar.date(from: DateComponents(
            year: parts[0],
            month: parts[1],
            day: parts[2]
        ))
    }

    static func providerDisplayName(_ provider: String) -> String {
        switch provider.lowercased() {
        case "codex": "Codex"
        case "claude": "Claude"
        default: provider
        }
    }

    static func optionalSum(_ values: [UInt64?]) -> UInt64? {
        let known = values.compactMap { $0 }
        guard !known.isEmpty else { return nil }
        return known.reduce(0) { $0.saturatingAdd($1) }
    }

    static func optionalAdd(_ left: UInt64?, _ right: UInt64?) -> UInt64? {
        guard left != nil || right != nil else { return nil }
        return (left ?? 0).saturatingAdd(right ?? 0)
    }

    private static func modelPricedTokens(_ model: TokenUsageModelTotal) -> UInt64 {
        model.pricedTokens ?? (model.estimatedCostUsdMicros == nil ? 0 : model.total)
    }

    private static func modelUnpricedTokens(_ model: TokenUsageModelTotal) -> UInt64 {
        model.unpricedTokens ?? (model.estimatedCostUsdMicros == nil ? model.total : 0)
    }

    private static func coveragePercent(priced: UInt64, unpriced: UInt64) -> String {
        let total = priced.saturatingAdd(unpriced)
        guard total > 0 else { return "0.0%" }
        return String(format: "%.1f%%", Double(priced) / Double(total) * 100)
    }

    private static func dashboardDay(
        date: Date,
        key: String,
        source: TokenUsageDayTotal?
    ) -> TokenDashboardDay {
        TokenDashboardDay(
            day: key,
            date: date,
            total: source?.total ?? 0,
            estimatedCostUsdMicros: source?.estimatedCostUsdMicros,
            pricedTokens: source?.pricedTokens ?? 0,
            unpricedTokens: source?.unpricedTokens ?? 0,
            messageCount: source?.messageCount ?? 0,
            byProvider: source?.byProvider ?? [],
            byModel: source?.byModel ?? []
        )
    }

    private static func breakdownSort(
        _ left: TokenDashboardBreakdownRow,
        _ right: TokenDashboardBreakdownRow
    ) -> Bool {
        if left.total != right.total { return left.total > right.total }
        return left.label < right.label
    }

    /// Uses a P95 cap plus logarithmic spacing so one genuine or suspect peak
    /// cannot flatten the other 364 days into the lightest bucket.
    private static func heatmapScale(_ values: [UInt64]) -> UInt64 {
        let positive = values.filter { $0 > 0 }.sorted()
        guard let maximum = positive.last else { return 0 }
        guard positive.count >= 20 else { return maximum }
        let index = min(
            positive.count - 1,
            max(0, Int(ceil(Double(positive.count) * 0.95)) - 1)
        )
        return max(1, positive[index])
    }

    private static func heatmapLevel(value: UInt64, scale: UInt64) -> Int {
        guard value > 0, scale > 0 else { return 0 }
        let ratio = log1p(Double(min(value, scale))) / log1p(Double(scale))
        if ratio >= 0.75 { return 4 }
        if ratio >= 0.5 { return 3 }
        if ratio >= 0.25 { return 2 }
        return 1
    }
}

struct TokenDashboardRenderInput: Equatable {
    let tokenUsage: TokenUsageTotals
    let tokenDecision: TokenUsageDecisionSummary
    let uiSettings: UISettings
    let referenceDay: Date

    init(
        tokenUsage: TokenUsageTotals,
        tokenDecision: TokenUsageDecisionSummary,
        uiSettings: UISettings,
        now: Date,
        calendar: Calendar = .current
    ) {
        self.tokenUsage = tokenUsage
        self.tokenDecision = tokenDecision
        self.uiSettings = uiSettings
        referenceDay = calendar.startOfDay(for: now)
    }
}

public struct TokenUsageDashboardView: View {
    @EnvironmentObject private var model: AppModel
    public init() {}

    public var body: some View {
        TokenUsageDashboardContent(
            input: TokenDashboardRenderInput(
                tokenUsage: model.tokenUsage,
                tokenDecision: model.tokenDecision,
                uiSettings: model.uiSettings,
                now: model.now
            ),
            client: model.client
        )
        .equatable()
    }
}

private struct TokenUsageDashboardContent: View, Equatable {
    let input: TokenDashboardRenderInput
    let client: RuntimeClient
    @Environment(\.locale) private var locale
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var period: TokenDashboardPeriod = .total
    @State private var page: TokenDashboardPage = .overview
    @State private var breakdown: TokenDashboardBreakdown = .provider
    @State private var range: TokenDashboardRange = .thirty
    @State private var heatmapMetric: TokenDashboardHeatmapMetric = .tokens
    @State private var selectedChartDate: Date?
    @State private var selectedCompositionRowID: String?
    @State private var qualityDetailsExpanded = false
    @State private var exportError: String?
    @State private var exporting = false

    nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.input == rhs.input
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            pageTabs
            ScrollView(.vertical, showsIndicators: true) {
                Group {
                    if page == .overview {
                        overview
                    } else {
                        trends
                    }
                }
                .padding(.horizontal, 24)
                .padding(.top, 18)
                .padding(.bottom, 26)
            }
        }
        .frame(minWidth: 820, idealWidth: 1180, minHeight: 620, idealHeight: 760)
        .background(TokenDashboardBackground())
        .preferredColorScheme(.dark)
        .task {
            await client.refreshSnapshot()
        }
        .alert(localized("导出失败", locale: locale), isPresented: Binding(
            get: { exportError != nil }, set: { if !$0 { exportError = nil } }
        )) {
            Button(localized("好", locale: locale)) { exportError = nil }
        } message: { Text(exportError ?? "") }
    }

    @MainActor
    private func exportUsage(csv: Bool) async {
        guard !exporting else { return }
        exporting = true
        defer { exporting = false }
        let (data, error) = await client.exportTokenUsage(csv: csv)
        guard let data else { exportError = error ?? localized("导出失败", locale: locale); return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [csv ? .commaSeparatedText : .json]
        panel.nameFieldStringValue = csv ? "actrealm-token-usage.csv" : "actrealm-token-usage.json"
        guard await panel.begin() == .OK, let url = panel.url else { return }
        do { try data.write(to: url, options: .atomic) }
        catch { exportError = error.localizedDescription }
    }

    private var header: some View {
        HStack(spacing: 13) {
            Image(systemName: "chart.xyaxis.line")
                .font(.system(size: 17, weight: .bold))
                .foregroundStyle(.white.opacity(0.92))
                .frame(width: 32, height: 32)
                .background(Color.cyan.opacity(0.14), in: RoundedRectangle(cornerRadius: 9))
            VStack(alignment: .leading, spacing: 2) {
                Text(localized("Token 仪表板", locale: locale))
                    .font(.system(size: 17, weight: .bold, design: .rounded))
                    .foregroundStyle(.white.opacity(0.94))
                Text(localized("本机 Agent 用量 · 实时更新", locale: locale))
                    .font(.system(size: 10.5, weight: .medium))
                    .foregroundStyle(.white.opacity(0.46))
            }
            Spacer()
            collectionBadge
            Menu {
                Button(localized("导出 Token CSV…", locale: locale)) { Task { await exportUsage(csv: true) } }
                Button(localized("导出 Token JSON…", locale: locale)) { Task { await exportUsage(csv: false) } }
            } label: {
                Image(systemName: "square.and.arrow.up")
            }
            .menuStyle(.borderlessButton)
            .frame(width: 30)
            .disabled(exporting)
            .help(localized("导出本机用量", locale: locale))
            .accessibilityLabel(Text(localized("导出本机用量", locale: locale)))
            Button {
                Task { await client.refreshSnapshot() }
            } label: {
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(TokenDashboardIconButtonStyle())
            .help(localized("刷新 Token 数据", locale: locale))
            Button {
                dismissWindow(id: "token-dashboard")
            } label: {
                Image(systemName: "xmark")
            }
            .buttonStyle(TokenDashboardIconButtonStyle())
            .help(localized("关闭窗口", locale: locale))
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 15)
        .background(.black.opacity(0.16))
        .overlay(alignment: .bottom) {
            Rectangle().fill(.white.opacity(0.08)).frame(height: 1)
        }
    }

    private var pageTabs: some View {
        HStack(spacing: 7) {
            ForEach(TokenDashboardPage.allCases) { item in
                Button(pageTitle(item)) {
                    page = item
                    selectedChartDate = nil
                }
                .buttonStyle(TokenDashboardTabStyle(selected: page == item))
            }
            Spacer()
            if page == .overview {
                periodPicker
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 11)
        .background(.black.opacity(0.08))
    }

    private var periodPicker: some View {
        HStack(spacing: 3) {
            ForEach(TokenDashboardPeriod.allCases) { item in
                Button(periodTitle(item)) {
                    period = item
                    selectedCompositionRowID = nil
                }
                    .buttonStyle(TokenDashboardSegmentStyle(selected: period == item))
            }
        }
        .padding(4)
        .background(.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 11))
        .overlay(RoundedRectangle(cornerRadius: 11).strokeBorder(.white.opacity(0.07)))
    }

    private var overview: some View {
        VStack(spacing: 18) {
            if !analyticsAreFinal { provisionalAnalyticsCard }
            if TokenDashboardPresentation.hasObservedData(input.tokenUsage) {
                summaryGrid
                if input.uiSettings.tokenUsageHeatmapVisible { heatmapCard }
                if input.uiSettings.tokenUsageComponentsVisible { compositionCard }
                breakdownCard(
                    title: breakdownTitle(breakdown),
                    rows: selectedBreakdownRows,
                    showsSelector: true
                )
                if input.uiSettings.tokenUsageTaskProjectVisible {
                    tokenAllocationGrid
                    tokenAttributionCard
                }
                if input.uiSettings.tokenUsageBurnRateVisible { tokenBurnRateCard }
                if input.uiSettings.tokenUsageAnomalyVisible || input.tokenUsage.suspectCount > 0 {
                    qualityCard
                }
            }
            privacyNote
        }
    }

    private var provisionalAnalyticsCard: some View {
        HStack(alignment: .top, spacing: 11) {
            Image(systemName: "clock.arrow.trianglehead.counterclockwise.rotate.90")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(TokenDashboardPalette.warning)
            VStack(alignment: .leading, spacing: 4) {
                Text(localized(
                    !TokenDashboardPresentation.hasObservedData(input.tokenUsage) ? "等待本机用量记录"
                        : input.tokenUsage.dataQuality == "suspect" ? "部分统计需要核对" : "当前显示已观测用量",
                    locale: locale
                ))
                    .font(.system(size: 12, weight: .bold))
                    .foregroundStyle(.white.opacity(0.82))
                Text(localized(
                    TokenDashboardPresentation.hasObservedData(input.tokenUsage)
                        ? "历史覆盖可能不完整，图表展示已读取的记录；空白日期不代表确认零用量。采集状态和定价覆盖分别标注。"
                        : "收到首批可用记录后会自动显示用量；未采到数据时不会显示为零。",
                    locale: locale
                ))
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundStyle(.white.opacity(0.46))
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(16)
        .tokenDashboardCard(cornerRadius: 14)
    }

    private var tokenAttributionCard: some View {
        let decision = input.tokenDecision
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(localized("项目与任务归因", locale: locale))
                        .font(.system(size: 13.5, weight: .bold))
                        .foregroundStyle(.white.opacity(0.9))
                    Text(localized(
                        "项目来自 Provider 会话元数据；任务只使用可验证会话与父子关系",
                        locale: locale
                    ))
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundStyle(.white.opacity(0.4))
                    .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 12)
                VStack(alignment: .trailing, spacing: 3) {
                    Text(decisionFreshnessText)
                        .font(.system(size: 9, weight: .bold, design: .monospaced))
                        .foregroundStyle(decision.freshness == "live"
                            ? TokenDashboardPalette.output
                            : TokenDashboardPalette.warning)
                    Text(localized("来源：本机事实账本", locale: locale))
                        .font(.system(size: 8.5, weight: .medium))
                        .foregroundStyle(.white.opacity(0.32))
                }
            }
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 16) {
                    projectAttributionLayer(decision)
                    Divider().frame(height: 36).overlay(.white.opacity(0.08))
                    taskAttributionLayer(decision)
                }
                VStack(spacing: 10) {
                    projectAttributionLayer(decision)
                    Divider().overlay(.white.opacity(0.08))
                    taskAttributionLayer(decision)
                }
            }
        }
        .padding(.horizontal, 17)
        .padding(.vertical, 14)
        .tokenDashboardCard(cornerRadius: 14)
    }

    private func projectAttributionLayer(_ decision: TokenUsageDecisionSummary) -> some View {
        attributionLayer(
            title: localized("项目", locale: locale),
            coverageBasisPoints: decision.projectAttributionCoverageBasisPoints,
            attributed: decision.projectAttributedTokens,
            unattributed: decision.projectUnattributedTokens,
            unassignedLabel: localized("项目未识别", locale: locale)
        )
    }

    private func taskAttributionLayer(_ decision: TokenUsageDecisionSummary) -> some View {
        attributionLayer(
            title: localized("任务", locale: locale),
            coverageBasisPoints: decision.taskAttributionCoverageBasisPoints,
            attributed: decision.taskAttributedTokens,
            unattributed: decision.taskUnattributedTokens,
            unassignedLabel: localized("任务不可恢复", locale: locale)
        )
    }

    private func attributionLayer(
        title: String,
        coverageBasisPoints: UInt64,
        attributed: UInt64,
        unattributed: UInt64,
        unassignedLabel: String
    ) -> some View {
        HStack(spacing: 10) {
            VStack(alignment: .trailing, spacing: 2) {
                Text(title.uppercased())
                    .font(.system(size: 8.5, weight: .bold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.32))
                Text(String(format: "%.1f%%", Double(coverageBasisPoints) / 100))
                    .font(.system(size: 15, weight: .bold, design: .rounded))
                    .foregroundStyle(.white.opacity(0.9))
                    .monospacedDigit()
            }
            decisionMetric(
                compactTokens(attributed),
                localized("已归因", locale: locale)
            )
            decisionMetric(
                compactTokens(unattributed),
                unassignedLabel,
                warning: unattributed > 0
            )
        }
    }

    private func decisionMetric(
        _ value: String,
        _ label: String,
        warning: Bool = false
    ) -> some View {
        VStack(alignment: .trailing, spacing: 2) {
            Text(value)
                .font(.system(size: 15, weight: .bold, design: .rounded))
                .foregroundStyle(warning
                    ? TokenDashboardPalette.warning
                    : .white.opacity(0.88))
                .monospacedDigit()
            Text(label)
                .font(.system(size: 8.5, weight: .semibold))
                .foregroundStyle(.white.opacity(0.34))
        }
        .frame(minWidth: 72, alignment: .trailing)
    }

    private var tokenAllocationGrid: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .top, spacing: 18) {
                projectAllocationCard
                taskAllocationCard
            }
            VStack(spacing: 18) {
                projectAllocationCard
                taskAllocationCard
            }
        }
    }

    private var projectAllocationCard: some View {
        allocationCard(
            title: localized("按项目", locale: locale),
            empty: localized("还没有可归因的项目用量", locale: locale),
            rows: input.tokenDecision.projectTotals.prefix(6).map {
                let detail = if $0.taskCount > 0 {
                    localizedFormat(
                        "%lld 个任务 · %lld 个会话",
                        locale: locale,
                        Int64($0.taskCount),
                        Int64($0.sessionCount)
                    )
                } else {
                    localizedFormat(
                        "%lld 个历史会话",
                        locale: locale,
                        Int64($0.sessionCount)
                    )
                }
                return ($0.project, $0.total, detail)
            }
        )
    }

    private var taskAllocationCard: some View {
        allocationCard(
            title: localized("按任务", locale: locale),
            empty: localized("还没有可归因的任务用量", locale: locale),
            rows: input.tokenDecision.taskTotals.prefix(6).map {
                (
                    $0.title ?? $0.project ?? $0.provider,
                    $0.total,
                    [$0.provider, $0.model].compactMap { $0 }.joined(separator: " · ")
                )
            }
        )
    }

    private func allocationCard(
        title: String,
        empty: String,
        rows: [(String, UInt64, String)]
    ) -> some View {
        VStack(alignment: .leading, spacing: 11) {
            Text(title.uppercased())
                .font(.system(size: 10.5, weight: .bold, design: .monospaced))
                .foregroundStyle(.white.opacity(0.48))
            if rows.isEmpty {
                tokenEmptyState(empty).frame(minHeight: 118)
            } else {
                let maximum = max(1, rows.map(\.1).max() ?? 1)
                ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                    VStack(alignment: .leading, spacing: 5) {
                        HStack(spacing: 8) {
                            Circle()
                                .fill(seriesColor(key: row.0, index: index))
                                .frame(width: 6, height: 6)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(row.0)
                                    .font(.system(size: 10.5, weight: .semibold))
                                    .foregroundStyle(.white.opacity(0.8))
                                    .lineLimit(1)
                                Text(row.2)
                                    .font(.system(size: 8, weight: .medium))
                                    .foregroundStyle(.white.opacity(0.3))
                                    .lineLimit(1)
                            }
                            Spacer()
                            Text(compactTokens(row.1))
                                .font(.system(size: 10.5, weight: .bold, design: .monospaced))
                                .foregroundStyle(.white.opacity(0.78))
                        }
                        GeometryReader { proxy in
                            Capsule()
                                .fill(.white.opacity(0.05))
                                .overlay(alignment: .leading) {
                                    Capsule()
                                        .fill(seriesColor(key: row.0, index: index).opacity(0.8))
                                        .frame(width: proxy.size.width
                                            * CGFloat(Double(row.1) / Double(maximum)))
                                }
                        }
                        .frame(height: 4)
                    }
                }
            }
        }
        .padding(17)
        .frame(maxWidth: .infinity, minHeight: 250, alignment: .topLeading)
        .tokenDashboardCard(cornerRadius: 16)
    }

    private var tokenBurnRateCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(localized("当前燃烧速度", locale: locale))
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white.opacity(0.9))
                    Text(localized(
                        "真实 5 分钟滑动窗口；异常只比较数值，不判断是否浪费",
                        locale: locale
                    ))
                    .font(.system(size: 9.5, weight: .medium))
                    .foregroundStyle(.white.opacity(0.38))
                }
                Spacer()
                if let threshold = input.tokenDecision.thresholdTokensPerMinute {
                    Text(localizedFormat(
                        "提醒阈值 %@ / 分钟",
                        locale: locale,
                        compactTokens(threshold)
                    ))
                    .font(.system(size: 8.5, weight: .bold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.38))
                }
            }
            if input.tokenDecision.burnRates.isEmpty {
                tokenEmptyState(localized("当前没有运行中的可采样任务", locale: locale))
                    .frame(minHeight: 95)
            } else {
                ForEach(input.tokenDecision.burnRates.prefix(6)) { burn in
                    HStack(spacing: 10) {
                        Circle()
                            .fill(burn.state == "elevated"
                                ? TokenDashboardPalette.warning
                                : TokenDashboardPalette.output)
                            .frame(width: 7, height: 7)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(burn.title ?? burn.project ?? burn.provider)
                                .font(.system(size: 10.5, weight: .semibold))
                                .foregroundStyle(.white.opacity(0.82))
                                .lineLimit(1)
                            Text(burnRateDetail(burn))
                                .font(.system(size: 8.5, weight: .medium))
                                .foregroundStyle(.white.opacity(0.34))
                                .lineLimit(1)
                        }
                        Spacer()
                        Text(burn.state == "collecting"
                            ? localized("采样中", locale: locale)
                            : localizedFormat(
                                "%@ / 分钟",
                                locale: locale,
                                compactTokens(burn.tokensPerMinute)
                            ))
                        .font(.system(size: 11, weight: .bold, design: .monospaced))
                        .foregroundStyle(burn.thresholdExceeded
                            ? TokenDashboardPalette.warning
                            : .white.opacity(0.76))
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(.white.opacity(0.035), in: RoundedRectangle(cornerRadius: 9))
                }
            }
        }
        .padding(18)
        .tokenDashboardCard(cornerRadius: 16)
    }

    private var decisionFreshnessText: String {
        switch input.tokenDecision.freshness {
        case "live": localized("实时", locale: locale)
        case "delayed": localized("延迟", locale: locale)
        case "stale": localized("已过期", locale: locale)
        default: localized("不可用", locale: locale)
        }
    }

    private func burnRateDetail(_ burn: TokenUsageBurnRate) -> String {
        guard burn.state != "collecting" else {
            return localizedFormat(
                "已采集 %lld 个实时样本",
                locale: locale,
                Int64(burn.sampleCount)
            )
        }
        let window = localizedFormat(
            "%lld 秒窗口 · 增加 %@ Token",
            locale: locale,
            Int64(burn.windowSeconds),
            compactTokens(burn.tokenDelta)
        )
        guard let baseline = burn.baselineTokensPerMinute,
              let ratio = burn.ratioBasisPoints
        else { return window }
        return localizedFormat(
            "%@ · 30 分钟基线 %@ / 分钟 · %.1f×",
            locale: locale,
            window,
            compactTokens(baseline),
            Double(ratio) / 10_000
        )
    }

    private var qualityCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Button {
                qualityDetailsExpanded.toggle()
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: input.tokenUsage.suspectCount > 0
                        ? "exclamationmark.triangle.fill"
                        : "info.circle.fill")
                    VStack(alignment: .leading, spacing: 2) {
                        Text(qualitySummaryTitle)
                            .font(.system(size: 11.5, weight: .bold))
                        Text(localized(
                            "点击查看本机审计原因；不会包含 Prompt、路径或命令",
                            locale: locale
                        ))
                        .font(.system(size: 9, weight: .medium))
                        .foregroundStyle(.white.opacity(0.42))
                    }
                    Spacer()
                    Image(systemName: qualityDetailsExpanded ? "chevron.up" : "chevron.down")
                        .font(.system(size: 9, weight: .bold))
                }
                .foregroundStyle(input.tokenUsage.suspectCount > 0
                    ? TokenDashboardPalette.warning
                    : .white.opacity(0.72))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if qualityDetailsExpanded {
                VStack(alignment: .leading, spacing: 7) {
                    ForEach(Array(input.tokenUsage.anomalies.prefix(8))) { anomaly in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Circle()
                                .fill(anomaly.severity == "suspect"
                                    ? TokenDashboardPalette.warning
                                    : .white.opacity(0.35))
                                .frame(width: 5, height: 5)
                            Text(anomalyDescription(anomaly))
                                .font(.system(size: 9.5, weight: .medium))
                                .foregroundStyle(.white.opacity(0.62))
                                .textSelection(.enabled)
                            Spacer(minLength: 0)
                        }
                    }
                    if input.tokenUsage.anomalyCount > 8 {
                        Text(localizedFormat(
                            "另有 %lld 项，可在 Token JSON 导出中查看",
                            locale: locale,
                            Int64(input.tokenUsage.anomalyCount - 8)
                        ))
                        .font(.system(size: 9, weight: .medium))
                        .foregroundStyle(.white.opacity(0.38))
                    }
                }
                .padding(.top, 2)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 13)
        .background(.black.opacity(0.13), in: RoundedRectangle(cornerRadius: 13))
        .overlay(
            RoundedRectangle(cornerRadius: 13)
                .strokeBorder(
                    input.tokenUsage.suspectCount > 0
                        ? TokenDashboardPalette.warning.opacity(0.32)
                        : .white.opacity(0.08),
                    lineWidth: 1
                )
        )
    }

    private var qualitySummaryTitle: String {
        if input.tokenUsage.suspectCount > 0 {
            return localizedFormat(
                "发现 %lld 项可疑数据",
                locale: locale,
                Int64(input.tokenUsage.suspectCount)
            )
        }
        if !analyticsAreFinal {
            return localized("历史覆盖未完成，只展示已观测数据", locale: locale)
        }
        return localized(input.tokenUsage.unpricedTokens > 0 ? "费用覆盖不完整，Token 总量仍可用" : "用量统计已核对", locale: locale)
    }

    private func anomalyDescription(_ anomaly: TokenUsageAnomaly) -> String {
        switch anomaly.code {
        case "daily_projection_mismatch":
            return localizedFormat(
                "逐日汇总与事实账本不一致：%@ / %@",
                locale: locale,
                compactTokens(anomaly.observed ?? 0),
                compactTokens(anomaly.expected ?? 0)
            )
        case "model_projection_mismatch":
            return localizedFormat(
                "模型汇总与事实账本不一致：%@ / %@",
                locale: locale,
                compactTokens(anomaly.observed ?? 0),
                compactTokens(anomaly.expected ?? 0)
            )
        case "negative_usage_value":
            return localizedFormat(
                "账本中发现 %lld 条负值记录",
                locale: locale,
                Int64(anomaly.observed ?? 0)
            )
        case "future_usage_day":
            return localizedFormat(
                "%@ 晚于本机当前日期，未参与峰值判断",
                locale: locale,
                anomaly.day ?? "—"
            )
        case "extreme_daily_jump":
            return localizedFormat(
                "%@ 的 %@ Token 超过稳健异常阈值，未参与峰值判断",
                locale: locale,
                anomaly.day ?? "—",
                compactTokens(anomaly.observed ?? 0)
            )
        case "unpriced_tokens":
            return localizedFormat(
                "%@ Token 缺少可靠模型价格；费用仅显示已定价下限",
                locale: locale,
                compactTokens(anomaly.observed ?? 0)
            )
        default:
            return anomaly.code
        }
    }

    private var summaryGrid: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 0) {
                ForEach(summaryItems) { item in
                    summaryMetric(item.value, item.label, detail: item.detail)
                }
            }
            .frame(minWidth: 1_008)
            LazyVGrid(
                columns: Array(repeating: GridItem(.flexible(), spacing: 0), count: 4),
                spacing: 0
            ) {
                ForEach(summaryItems) { item in
                    summaryMetric(item.value, item.label, detail: item.detail)
                }
            }
            LazyVGrid(
                columns: Array(repeating: GridItem(.flexible(), spacing: 0), count: 2),
                spacing: 0
            ) {
                ForEach(summaryItems) { item in
                    summaryMetric(item.value, item.label, detail: item.detail)
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 15, style: .continuous))
        .background(.black.opacity(0.15), in: RoundedRectangle(cornerRadius: 15))
        .overlay(
            RoundedRectangle(cornerRadius: 15)
                .strokeBorder(.white.opacity(0.09), lineWidth: 1)
        )
    }

    private func summaryMetric(
        _ value: String,
        _ label: String,
        detail: String? = nil
    ) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(value)
                .font(.system(size: 18, weight: .bold, design: .rounded))
                .foregroundStyle(.white.opacity(0.92))
                .lineLimit(1)
                .minimumScaleFactor(0.6)
                .monospacedDigit()
            Text(localized(label, locale: locale).uppercased())
                .font(.system(size: 9.5, weight: .bold, design: .monospaced))
                .foregroundStyle(.white.opacity(0.42))
                .lineLimit(1)
            Text(detail ?? " ")
                .font(.system(size: 8.5, weight: .medium))
                .foregroundStyle(.white.opacity(0.27))
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, minHeight: 75, alignment: .leading)
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .overlay(alignment: .trailing) {
            Rectangle().fill(.white.opacity(0.065)).frame(width: 1)
        }
    }

    private var heatmapCard: some View {
        VStack(alignment: .leading, spacing: 13) {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(localized("Token 活跃度", locale: locale))
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white.opacity(0.9))
                    Text(localized("过去 12 个月 · 周日对齐", locale: locale))
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundStyle(.white.opacity(0.36))
                }
                Spacer()
                segmentedControl(
                    values: TokenDashboardHeatmapMetric.allCases,
                    selection: heatmapMetric,
                    title: heatmapMetricTitle,
                    action: { heatmapMetric = $0 }
                )
            }
            TokenUsageHeatmap(
                calendar: TokenDashboardPresentation.heatmapCalendar(
                    totals: input.tokenUsage,
                    now: input.referenceDay,
                    calendar: Calendar.current
                ),
                metric: heatmapMetric,
                locale: locale
            )
            HStack(spacing: 5) {
                Spacer()
                Text(localized("少", locale: locale))
                ForEach(0..<5, id: \.self) { level in
                    RoundedRectangle(cornerRadius: 2)
                        .fill(TokenUsageHeatmap.color(level: level, metric: heatmapMetric))
                        .frame(width: 11, height: 11)
                }
                Text(localized("多", locale: locale))
            }
            .font(.system(size: 8.5, weight: .medium))
            .foregroundStyle(.white.opacity(0.34))
        }
        .padding(18)
        .tokenDashboardCard(cornerRadius: 16)
    }

    private func breakdownCard(
        title: String,
        rows: [TokenDashboardBreakdownRow],
        showsSelector: Bool = false
    ) -> some View {
        VStack(alignment: .leading, spacing: 13) {
            HStack {
                Text(title.uppercased())
                    .font(.system(size: 10.5, weight: .bold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.48))
                Spacer()
                if showsSelector {
                    segmentedControl(
                        values: TokenDashboardBreakdown.allCases,
                        selection: breakdown,
                        title: breakdownTitle,
                        action: {
                            breakdown = $0
                            selectedCompositionRowID = nil
                        }
                    )
                }
                Text(compactTokens(rows.reduce(UInt64(0)) {
                    $0.saturatingAdd($1.total)
                }))
                .font(.system(size: 10, weight: .bold, design: .monospaced))
                .foregroundStyle(.white.opacity(0.34))
            }
            if rows.isEmpty {
                tokenEmptyState(
                    localized("这段时间还没有可验证的拆分数据", locale: locale)
                )
                .frame(minHeight: 110)
            } else {
                ForEach(Array(rows.prefix(5).enumerated()), id: \.element.id) {
                    index, row in
                    breakdownRow(
                        row,
                        maximum: max(1, rows.first?.total ?? 1),
                        color: seriesColor(key: row.label, index: index)
                    )
                }
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, minHeight: 218, alignment: .topLeading)
        .tokenDashboardCard(cornerRadius: 16)
    }

    private func breakdownRow(
        _ row: TokenDashboardBreakdownRow,
        maximum: UInt64,
        color: Color
    ) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            HStack(spacing: 8) {
                Circle().fill(color).frame(width: 7, height: 7)
                Text(row.label)
                    .font(.system(size: 11.5, weight: .semibold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.84))
                    .lineLimit(1)
                Spacer()
                Text(compactTokens(row.total))
                    .font(.system(size: 11.5, weight: .bold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.84))
                Text(percent(row.total, of: period.value(in: input.tokenUsage)))
                    .font(.system(size: 9.5, weight: .semibold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.36))
                    .frame(width: 48, alignment: .trailing)
            }
            GeometryReader { proxy in
                Capsule()
                    .fill(.white.opacity(0.06))
                    .overlay(alignment: .leading) {
                        Capsule()
                            .fill(color.opacity(0.88))
                            .frame(
                                width: proxy.size.width
                                    * CGFloat(Double(row.total) / Double(maximum))
                            )
                    }
            }
            .frame(height: 5)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 7)
        .background(
            selectedCompositionRowID == row.id
                ? color.opacity(0.10)
                : Color.clear,
            in: RoundedRectangle(cornerRadius: 8, style: .continuous)
        )
        .contentShape(Rectangle())
        .onTapGesture {
            withAnimation(.easeOut(duration: 0.16)) {
                selectedCompositionRowID = selectedCompositionRowID == row.id
                    ? nil
                    : row.id
            }
        }
        .help(localized(
            "点击聚焦查看该项的输入、缓存与输出组成",
            locale: locale
        ))
        .accessibilityAddTraits(.isButton)
    }

    private var compositionCard: some View {
        let composition = selectedComposition
        return VStack(alignment: .leading, spacing: 15) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(localized("Token 组成", locale: locale))
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white.opacity(0.9))
                    Text(compositionSubtitle)
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundStyle(.white.opacity(0.38))
                }
                Spacer()
                if let hitRate = composition.cacheHitRate {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(String(format: "%.1f%%", hitRate * 100))
                            .font(.system(size: 18, weight: .bold, design: .rounded))
                            .foregroundStyle(TokenDashboardPalette.cacheRead)
                            .monospacedDigit()
                        Text(localized("缓存命中率", locale: locale))
                            .font(.system(size: 8.5, weight: .bold, design: .monospaced))
                            .foregroundStyle(.white.opacity(0.38))
                    }
                }
                if selectedCompositionRow != nil {
                    Button(localized("查看全部", locale: locale)) {
                        withAnimation(.easeOut(duration: 0.16)) {
                            selectedCompositionRowID = nil
                        }
                    }
                    .buttonStyle(TokenDashboardSegmentStyle(selected: false))
                }
            }

            if !composition.hasComponentData {
                tokenEmptyState(localized(
                    "这段时间没有可验证的输入、缓存和输出明细",
                    locale: locale
                ))
                .frame(minHeight: 120)
            } else {
                GeometryReader { proxy in
                    let total = max(1, composition.displayTotal)
                    HStack(spacing: 3) {
                        ForEach(compositionParts) { part in
                            if part.value > 0 {
                                RoundedRectangle(cornerRadius: 4, style: .continuous)
                                    .fill(part.color)
                                    .frame(
                                        width: max(
                                            3,
                                            proxy.size.width
                                                * CGFloat(Double(part.value) / Double(total))
                                        )
                                    )
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .clipShape(RoundedRectangle(cornerRadius: 4, style: .continuous))
                }
                .frame(height: 10)

                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 145), spacing: 10)],
                    alignment: .leading,
                    spacing: 10
                ) {
                    ForEach(compositionParts) { part in
                        compositionMetric(part, total: composition.displayTotal)
                    }
                }

                HStack(spacing: 8) {
                    if composition.reasoningTokens > 0 {
                        Image(systemName: "brain.head.profile")
                            .foregroundStyle(TokenDashboardPalette.reasoning)
                        Text(localizedFormat(
                            "其中推理输出 %@ · 已包含在输出中，不重复计入总量",
                            locale: locale,
                            compactTokens(composition.reasoningTokens)
                        ))
                    }
                    if composition.hasInconsistentTotals {
                        Spacer(minLength: 8)
                        Label(
                            localized("部分 Provider 组成与总量不完全闭合", locale: locale),
                            systemImage: "exclamationmark.triangle.fill"
                        )
                        .foregroundStyle(TokenDashboardPalette.warning)
                    }
                }
                .font(.system(size: 9, weight: .medium))
                .foregroundStyle(.white.opacity(0.42))
            }
        }
        .padding(18)
        .tokenDashboardCard(cornerRadius: 16, emphasized: true)
    }

    private func compositionMetric(
        _ part: TokenDashboardComponentPart,
        total: UInt64
    ) -> some View {
        HStack(spacing: 9) {
            RoundedRectangle(cornerRadius: 3, style: .continuous)
                .fill(part.color)
                .frame(width: 9, height: 26)
            VStack(alignment: .leading, spacing: 3) {
                Text(part.label)
                    .font(.system(size: 9.5, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.48))
                Text(compactTokens(part.value))
                    .font(.system(size: 13, weight: .bold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.88))
                    .monospacedDigit()
            }
            Spacer(minLength: 4)
            Text(percent(part.value, of: total))
                .font(.system(size: 9, weight: .semibold, design: .monospaced))
                .foregroundStyle(.white.opacity(0.32))
        }
        .padding(10)
        .background(.black.opacity(0.13), in: RoundedRectangle(cornerRadius: 10))
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .strokeBorder(.white.opacity(0.055), lineWidth: 1)
        )
    }

    private var trends: some View {
        VStack(spacing: 16) {
            if !analyticsAreFinal { provisionalAnalyticsCard }
            if TokenDashboardPresentation.hasObservedData(input.tokenUsage) {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 10) {
                        trendControlGroups
                        Spacer()
                        rangePicker
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        trendControlGroups
                        rangePicker
                    }
                }
                trendChartCard
            }
            privacyNote
        }
    }

    @ViewBuilder
    private var trendControlGroups: some View {
        segmentedControl(
            values: TokenDashboardBreakdown.allCases,
            selection: breakdown,
            title: breakdownTitle,
            action: {
                breakdown = $0
                selectedChartDate = nil
            }
        )
    }

    private var rangePicker: some View {
        HStack(spacing: 3) {
            ForEach(TokenDashboardRange.allCases) { item in
                Button(rangeTitle(item)) {
                    range = item
                    selectedChartDate = nil
                }
                .buttonStyle(TokenDashboardSegmentStyle(selected: range == item))
            }
        }
        .padding(4)
        .background(.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 11))
    }

    private func segmentedControl<Value: Identifiable & Equatable>(
        values: [Value],
        selection: Value,
        title: @escaping (Value) -> String,
        action: @escaping (Value) -> Void
    ) -> some View {
        HStack(spacing: 3) {
            ForEach(values) { item in
                Button(title(item)) { action(item) }
                    .buttonStyle(TokenDashboardSegmentStyle(selected: selection == item))
            }
        }
        .padding(4)
        .background(.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 11))
    }

    private var trendChartCard: some View {
        VStack(alignment: .leading, spacing: 15) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(localized("每日 Token 趋势", locale: locale))
                        .font(.system(size: 14, weight: .bold))
                        .foregroundStyle(.white.opacity(0.9))
                    Text(trendCoverageText)
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundStyle(.white.opacity(0.37))
                }
                Spacer()
                if let selected = selectedChartSummary {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(shortDate(selected.date))
                            .font(.system(size: 9.5, weight: .semibold))
                            .foregroundStyle(.white.opacity(0.44))
                        Text(selected.total.map(compactTokens) ?? localized("该日未采到用量记录", locale: locale))
                            .font(.system(size: 14, weight: .bold, design: .monospaced))
                            .foregroundStyle(.white.opacity(0.9))
                    }
                }
            }
            dailyBarsChart
        }
        .padding(20)
        .tokenDashboardCard(cornerRadius: 16, emphasized: true)
    }

    @ViewBuilder
    private var dailyBarsChart: some View {
        let points = trendPoints
        let summaries = TokenDashboardPresentation.seriesSummaries(points: points)
        let series = summaries.map(\.series)
        if points.isEmpty {
            tokenEmptyState(localized("所选时间范围暂无 Token 数据", locale: locale))
                .frame(height: 370)
        } else {
            Chart {
                ForEach(points) { point in
                    BarMark(
                        x: .value("Day", point.date, unit: .day),
                        y: .value("Tokens", point.value)
                    )
                    .foregroundStyle(by: .value("Series", point.series))
                    .cornerRadius(1.5)
                }
                if let selectedChartDate {
                    RuleMark(x: .value("Selected", selectedChartDate, unit: .day))
                        .foregroundStyle(.white.opacity(0.32))
                        .lineStyle(.init(lineWidth: 1, dash: [4, 4]))
                }
            }
            .chartForegroundStyleScale(
                domain: series,
                range: series.enumerated().map {
                    seriesColor(key: $0.element, index: $0.offset)
                }
            )
            .id("\(breakdown.rawValue):\(range.rawValue)")
            .chartXScale(domain: chartDomain(days: trendDays))
            .chartLegend(.hidden)
            .tokenDashboardAxes(desiredXCount: desiredXAxisCount, locale: locale)
            .chartXSelection(value: $selectedChartDate)
            .frame(minHeight: 320)
            customLegend(summaries)
        }
    }

    private func customLegend(
        _ summaries: [TokenDashboardSeriesSummary]
    ) -> some View {
        LazyVGrid(
            columns: [GridItem(.adaptive(minimum: 190), spacing: 12)],
            alignment: .leading,
            spacing: 9
        ) {
            ForEach(Array(summaries.enumerated()), id: \.element.id) {
                index, summary in
                HStack(spacing: 8) {
                    RoundedRectangle(cornerRadius: 2)
                        .fill(seriesColor(key: summary.series, index: index))
                        .frame(width: 10, height: 10)
                    Text(summary.series)
                        .font(.system(size: 10, weight: .semibold, design: .monospaced))
                        .foregroundStyle(.white.opacity(0.68))
                        .lineLimit(1)
                    Spacer(minLength: 6)
                    Text(compactTokens(summary.total))
                        .font(.system(size: 10, weight: .bold, design: .monospaced))
                        .foregroundStyle(.white.opacity(0.72))
                    Text(String(format: "%.1f%%", summary.share * 100))
                        .font(.system(size: 9.5, weight: .semibold, design: .monospaced))
                        .foregroundStyle(.white.opacity(0.36))
                        .frame(width: 46, alignment: .trailing)
                }
            }
        }
        .padding(.top, 2)
    }

    private var privacyNote: some View {
        HStack(spacing: 8) {
            Image(systemName: "lock.shield")
            Text(localized(
                "用量只保存在本机；模型价格来自 Models.dev，费用为 API 等价值",
                locale: locale
            ))
        }
        .font(.system(size: 9.5, weight: .medium))
        .foregroundStyle(.white.opacity(0.34))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var collectionBadge: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(analyticsAreFinal ? TokenDashboardPalette.up : TokenDashboardPalette.warning)
                .frame(width: 6, height: 6)
            Text(collectionStatus)
        }
        .font(.system(size: 9.5, weight: .semibold))
        .foregroundStyle(.white.opacity(0.54))
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .background(.white.opacity(0.06), in: Capsule())
    }

    private var selectedBreakdownRows: [TokenDashboardBreakdownRow] {
        breakdown == .provider ? providerRows : modelRows
    }

    private var providerRows: [TokenDashboardBreakdownRow] {
        TokenDashboardPresentation.providerRows(
            totals: input.tokenUsage,
            period: period,
            now: input.referenceDay,
            calendar: Calendar.current
        )
    }

    private var selectedCompositionRow: TokenDashboardBreakdownRow? {
        guard let selectedCompositionRowID else { return nil }
        return (providerRows + modelRows).first { $0.id == selectedCompositionRowID }
    }

    private var selectedComposition: TokenDashboardComposition {
        TokenDashboardPresentation.composition(
            rows: selectedCompositionRow.map { [$0] } ?? providerRows
        )
    }

    private var compositionSubtitle: String {
        if let selectedCompositionRow {
            return localizedFormat(
                "已聚焦 %@ · 再次点击该行可返回全部",
                locale: locale,
                selectedCompositionRow.label
            )
        }
        return localizedFormat(
            "%@ · Codex 与 Claude 已统一为不重叠口径",
            locale: locale,
            periodMetricTitle
        )
    }

    private var compositionParts: [TokenDashboardComponentPart] {
        let composition = selectedComposition
        var parts = [
            TokenDashboardComponentPart(
                id: "uncached-input",
                label: localized("未命中输入", locale: locale),
                value: composition.uncachedInputTokens,
                color: TokenDashboardPalette.uncachedInput
            ),
            TokenDashboardComponentPart(
                id: "cache-read",
                label: localized("缓存读取", locale: locale),
                value: composition.cacheReadTokens,
                color: TokenDashboardPalette.cacheRead
            ),
            TokenDashboardComponentPart(
                id: "cache-create",
                label: localized("缓存写入", locale: locale),
                value: composition.cacheCreationTokens,
                color: TokenDashboardPalette.cacheCreation
            ),
            TokenDashboardComponentPart(
                id: "output",
                label: localized("输出", locale: locale),
                value: composition.outputTokens,
                color: TokenDashboardPalette.output
            ),
        ]
        if composition.unclassifiedTokens > 0 {
            parts.append(TokenDashboardComponentPart(
                id: "unclassified",
                label: localized("未分类", locale: locale),
                value: composition.unclassifiedTokens,
                color: .white.opacity(0.3)
            ))
        }
        return parts
    }

    private var modelRows: [TokenDashboardBreakdownRow] {
        TokenDashboardPresentation.modelRows(
            totals: input.tokenUsage,
            period: period,
            now: input.referenceDay,
            calendar: Calendar.current
        )
    }

    private var selectedPricing: TokenDashboardPricingSummary {
        TokenDashboardPresentation.selectedPricingSummary(
            totals: input.tokenUsage,
            period: period,
            now: input.referenceDay,
            calendar: Calendar.current
        )
    }

    private var summaryItems: [TokenDashboardStat] {
        var items = [
            TokenDashboardStat(
                label: analyticsAreFinal ? "总 Token" : "已观测 Token",
                value: compactTokens(period.value(in: input.tokenUsage)),
                detail: analyticsAreFinal
                    ? periodMetricTitle
                    : localized("历史覆盖未完成", locale: locale)
            ),
        ]
        if input.uiSettings.tokenUsageCostVisible {
            items.append(TokenDashboardStat(
                label: analyticsAreFinal ? "API 等价值" : "已观测 API 等价值",
                value: selectedPricing.isPartial
                    ? localizedFormat(
                        "至少 %@",
                        locale: locale,
                        formattedCost(selectedPricing.estimatedCostUsdMicros)
                    )
                    : formattedCost(selectedPricing.estimatedCostUsdMicros),
                detail: selectedPricing.totalTokens > 0
                    ? localizedFormat(
                        "定价覆盖 %@ · 非订阅账单",
                        locale: locale,
                        percent(
                            selectedPricing.pricedTokens,
                            of: selectedPricing.totalTokens
                        )
                    )
                    : localized("暂无费用数据", locale: locale)
            ))
            if period == .total, !input.tokenUsage.pricingSources.isEmpty {
                items.append(TokenDashboardStat(
                    label: "价格依据",
                    value: pricingSourceSummary,
                    detail: localized("历史费用按写入来源冻结", locale: locale)
                ))
            }
        }
        items.append(contentsOf: [
            TokenDashboardStat(
                label: "活跃天数",
                value: String(selectedActiveDays),
                detail: localized("有 Token 记录", locale: locale)
            ),
            TokenDashboardStat(
                label: "当前连续",
                value: String(input.tokenUsage.currentStreak),
                detail: localized("天", locale: locale)
            ),
        ])
        if input.uiSettings.tokenUsageExecutionTimeVisible {
            items.append(TokenDashboardStat(
                label: "Agent 执行时间",
                value: formattedDuration(period.executionTime(in: input.tokenUsage)),
                detail: localized(
                    "仅累计思考、工具运行与上下文压缩；等待不计入，并发任务相加",
                    locale: locale
                )
            ))
        }
        if input.uiSettings.tokenUsageObservedTimeVisible {
            items.append(TokenDashboardStat(
                label: "任务观测时间",
                value: formattedDuration(period.observedTime(in: input.tokenUsage)),
                detail: localized("从 Turn 开始到最后事件，包含等待", locale: locale)
            ))
        }
        if analyticsAreFinal {
            items.append(contentsOf: [
                TokenDashboardStat(
                    label: "峰值日",
                    value: compactTokens(selectedPeak.total),
                    detail: selectedPeak.day.map(shortDayKey)
                ),
                TokenDashboardStat(
                    label: "常用模型",
                    value: selectedTopModel ?? "—",
                    detail: selectedTopProvider
                ),
            ])
        }
        items.append(TokenDashboardStat(
            label: "用量记录",
            value: exactTokens(selectedMessages),
            detail: nil
        ))
        return items
    }

    private var analyticsAreFinal: Bool {
        TokenDashboardPresentation.analyticsAreFinal(input.tokenUsage)
    }

    private var pricingSourceSummary: String {
        let sources = input.tokenUsage.pricingSources
        guard sources.count == 1, let source = sources.first else {
            return localizedFormat("%lld 个来源", locale: locale, Int64(sources.count))
        }
        if source.source == "claude_transcript_cost" {
            return localized("Claude 日志费用", locale: locale)
        }
        if source.source == "legacy_pre_v29" {
            return localized("旧版费用已冻结", locale: locale)
        }
        let date = source.source.split(separator: "_").last.map(String.init)
        if source.source.hasPrefix("models_dev_api_") {
            return "Models.dev"
        }
        if source.source.hasPrefix("models_dev_") {
            return ["Models.dev", date].compactMap { $0 }.joined(separator: " · ")
        }
        if source.source.hasPrefix("openai_") {
            return ["OpenAI", date].compactMap { $0 }.joined(separator: " · ")
        }
        if source.source.hasPrefix("anthropic_") {
            return ["Anthropic", date].compactMap { $0 }.joined(separator: " · ")
        }
        return source.source
    }

    private var selectedDays: [TokenUsageDayTotal] {
        TokenDashboardPresentation.periodDays(
            totals: input.tokenUsage,
            period: period,
            now: input.referenceDay,
            calendar: Calendar.current
        )
    }

    private var selectedActiveDays: UInt64 {
        period == .total
            ? input.tokenUsage.activeDays
            : UInt64(selectedDays.filter { $0.total > 0 }.count)
    }

    private var selectedPeak: (day: String?, total: UInt64) {
        if period == .total {
            return (input.tokenUsage.peakDay, input.tokenUsage.peakDayTotal)
        }
        let peak = selectedDays.max {
            if $0.total != $1.total { return $0.total < $1.total }
            return $0.day < $1.day
        }
        return (peak?.day, peak?.total ?? 0)
    }

    private var selectedTopModel: String? { modelRows.first?.label }
    private var selectedTopProvider: String? {
        guard let first = modelRows.first else { return nil }
        return TokenDashboardPresentation.providerDisplayName(first.provider)
    }
    private var selectedMessages: UInt64 {
        if period == .total {
            return input.tokenUsage.messageCount
        }
        return selectedDays.reduce(UInt64(0)) { $0.saturatingAdd($1.messageCount) }
    }

    private var trendDays: [TokenDashboardDay] {
        TokenDashboardPresentation.rangeDays(
            totals: input.tokenUsage,
            range: range,
            now: input.referenceDay,
            calendar: Calendar.current
        )
    }

    private var trendActiveDays: [TokenDashboardDay] {
        TokenDashboardPresentation.activeRangeDays(
            totals: input.tokenUsage,
            range: range,
            now: input.referenceDay,
            calendar: Calendar.current
        )
    }

    private var trendPoints: [TokenDashboardSeriesPoint] {
        TokenDashboardPresentation.seriesPoints(days: trendDays, breakdown: breakdown)
    }

    private var selectedChartSummary: (date: Date, total: UInt64?)? {
        guard let selectedChartDate else { return nil }
        guard let day = trendDays.min(by: {
            abs($0.date.timeIntervalSince(selectedChartDate))
                < abs($1.date.timeIntervalSince(selectedChartDate))
        }) else { return nil }
        return (day.date, input.tokenUsage.recentDays.contains { $0.day == day.day } ? day.total : nil)
    }

    private func chartDomain(days: [TokenDashboardDay]) -> ClosedRange<Date> {
        let fallback = input.referenceDay
        let first = days.first?.date ?? fallback
        let last = days.last?.date ?? fallback
        if first == last {
            return first.addingTimeInterval(-43_200)...last.addingTimeInterval(43_200)
        }
        return first...last.addingTimeInterval(43_200)
    }

    private var desiredXAxisCount: Int {
        switch range {
        case .seven: 7
        case .thirty: 6
        case .ninety: 7
        case .year, .all: 8
        }
    }

    private var trendCoverageText: String {
        guard let first = trendDays.first, let last = trendDays.last else {
            return localized("暂无历史数据", locale: locale)
        }
        let active = trendActiveDays.filter { $0.total > 0 }.count
        return localizedFormat(
            "%@ – %@ · %lld 个活跃日",
            locale: locale,
            shortDate(first.date),
            shortDate(last.date),
            Int64(active)
        )
    }

    private var collectionStatus: String {
        switch input.tokenUsage.dataQuality {
        case "verified":
            return localized("账本已验证", locale: locale)
        case "rebuilding":
            return localized("历史重建中", locale: locale)
        case "partial":
            return localized("历史数据部分可用", locale: locale)
        case "suspect":
            return localized("数据需要复核", locale: locale)
        case "unavailable":
            return localized("数据暂不可用", locale: locale)
        default:
            return localized("等待数据", locale: locale)
        }
    }

    private var periodMetricTitle: String {
        switch period {
        case .day: localized("今日", locale: locale)
        case .month: localized("本月", locale: locale)
        case .total: localized("全部历史", locale: locale)
        }
    }

    private func pageTitle(_ item: TokenDashboardPage) -> String {
        switch item {
        case .overview: localized("总览", locale: locale)
        case .trends: localized("趋势", locale: locale)
        }
    }

    private func periodTitle(_ item: TokenDashboardPeriod) -> String {
        switch item {
        case .day: localized("日", locale: locale)
        case .month: localized("月", locale: locale)
        case .total: localized("总计", locale: locale)
        }
    }

    private func breakdownTitle(_ item: TokenDashboardBreakdown) -> String {
        switch item {
        case .provider: localized("工具来源", locale: locale)
        case .model: localized("按模型", locale: locale)
        }
    }

    private func heatmapMetricTitle(_ item: TokenDashboardHeatmapMetric) -> String {
        switch item {
        case .tokens: "Tokens"
        case .cost: localized("费用", locale: locale)
        }
    }

    private func rangeTitle(_ item: TokenDashboardRange) -> String {
        switch item {
        case .seven: localized("7 天", locale: locale)
        case .thirty: localized("30 天", locale: locale)
        case .ninety: localized("90 天", locale: locale)
        case .year: localized("1 年", locale: locale)
        case .all: localized("全部", locale: locale)
        }
    }

    private func exactTokens(_ value: UInt64) -> String {
        NumberFormatter.localizedString(from: NSNumber(value: value), number: .decimal)
    }

    private func compactTokens(_ value: UInt64) -> String {
        TokenDashboardPresentation.tokenText(value, unitStyle: input.uiSettings.tokenUsageUnitStyle, locale: locale)
    }

    private func formattedCost(_ micros: UInt64?) -> String {
        tokenDashboardFormattedCost(micros, locale: locale)
    }

    private func formattedDuration(_ seconds: UInt64) -> String {
        if seconds == 0 { return "—" }
        let hours = seconds / 3_600
        let minutes = (seconds % 3_600) / 60
        if hours > 0 { return "\(hours)h \(minutes)m" }
        return "\(minutes)m"
    }

    private func percent(_ value: UInt64, of total: UInt64) -> String {
        guard total > 0 else { return "0.0%" }
        return String(format: "%.1f%%", Double(value) / Double(total) * 100)
    }

    private func shortDate(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.setLocalizedDateFormatFromTemplate("MMM d")
        return formatter.string(from: date)
    }

    private func shortDayKey(_ day: String) -> String {
        guard let date = TokenDashboardPresentation.date(
            from: day,
            calendar: Calendar.current
        ) else { return day }
        return shortDate(date)
    }

    private func seriesColor(key: String, index: Int) -> Color {
        let normalized = key.lowercased()
        if normalized.contains("codex") || normalized.contains("gpt") {
            return TokenDashboardPalette.series[0]
        }
        if normalized.contains("claude") {
            return TokenDashboardPalette.series[1]
        }
        return TokenDashboardPalette.series[index % TokenDashboardPalette.series.count]
    }

    private func tokenEmptyState(_ text: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: "chart.bar.xaxis")
                .font(.system(size: 20, weight: .medium))
                .foregroundStyle(.white.opacity(0.22))
            Text(text)
                .font(.system(size: 10.5, weight: .medium))
                .foregroundStyle(.white.opacity(0.35))
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct TokenUsageHeatmap: View {
    let calendar: TokenDashboardHeatmapCalendar
    let metric: TokenDashboardHeatmapMetric
    let locale: Locale

    @State private var hoveredDay: String?

    var body: some View {
        GeometryReader { proxy in
            let labelWidth: CGFloat = 24
            let gap: CGFloat = 3
            let usable = max(1, proxy.size.width - labelWidth)
            let cell = min(
                14,
                max(7, (usable - CGFloat(max(0, calendar.weeks - 1)) * gap)
                    / CGFloat(max(1, calendar.weeks)))
            )
            let pitch = cell + gap
            let tooltipWidth = max(1, min(240, proxy.size.width - 16))
            ZStack(alignment: .topLeading) {
                ForEach(calendar.monthLabels) { label in
                    Text(monthLabel(label.date))
                        .font(.system(size: 8.5, weight: .semibold))
                        .foregroundStyle(.white.opacity(0.36))
                        .offset(
                            x: labelWidth + CGFloat(label.column) * pitch,
                            y: 0
                        )
                }
                weekdayLabel("一", row: 1, pitch: pitch)
                weekdayLabel("三", row: 3, pitch: pitch)
                weekdayLabel("五", row: 5, pitch: pitch)
                ForEach(calendar.cells) { item in
                    RoundedRectangle(cornerRadius: max(2, cell * 0.18))
                        .fill(item.included
                            ? Self.color(level: item.level(for: metric), metric: metric)
                            : Color.clear)
                        .frame(width: cell, height: cell)
                        .overlay {
                            if item.included {
                                RoundedRectangle(cornerRadius: max(2, cell * 0.18))
                                    .strokeBorder(
                                        .white.opacity(hoveredDay == item.day ? 0.62 : 0.035),
                                        lineWidth: hoveredDay == item.day ? 1.25 : 0.5
                                    )
                                if metric == .cost,
                                   item.tokens > 0,
                                   item.unpricedTokens > 0 {
                                    RoundedRectangle(cornerRadius: max(2, cell * 0.18))
                                        .strokeBorder(
                                            Color.purple.opacity(0.66),
                                            style: StrokeStyle(
                                                lineWidth: 0.8,
                                                dash: [max(1.5, cell * 0.22), 1.4]
                                            )
                                        )
                                }
                            }
                        }
                        .scaleEffect(hoveredDay == item.day ? 1.12 : 1)
                        .contentShape(Rectangle())
                        .offset(
                            x: labelWidth + CGFloat(item.column) * pitch,
                            y: 19 + CGFloat(item.weekday) * pitch
                        )
                        .allowsHitTesting(item.included)
                        .onHover { isInside in
                            if isInside {
                                hoveredDay = item.day
                            } else if hoveredDay == item.day {
                                hoveredDay = nil
                            }
                        }
                        .accessibilityLabel(helpText(item))
                }
                if let hoveredDay,
                   let item = calendar.cells.first(where: { $0.day == hoveredDay }) {
                    let cellCenterX = labelWidth
                        + CGFloat(item.column) * pitch
                        + cell / 2
                    let tooltipX = min(
                        max(tooltipWidth / 2, cellCenterX),
                        proxy.size.width - tooltipWidth / 2
                    )
                    let cellTop = 19 + CGFloat(item.weekday) * pitch
                    let tooltipY = item.weekday <= 2
                        ? cellTop + cell + 20
                        : cellTop - 17
                    heatmapTooltip(item, width: tooltipWidth)
                        .position(x: tooltipX, y: tooltipY)
                        .zIndex(10)
                        .allowsHitTesting(false)
                }
            }
        }
        .frame(height: 19 + 7 * 17)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(localized("Token 活跃度", locale: locale))
    }

    static func color(
        level: Int,
        metric: TokenDashboardHeatmapMetric
    ) -> Color {
        let palette: [Color]
        if metric == .tokens {
            palette = [
                .white.opacity(0.055),
                Color(red: 0.10, green: 0.29, blue: 0.43),
                Color(red: 0.10, green: 0.46, blue: 0.60),
                Color(red: 0.08, green: 0.65, blue: 0.73),
                Color(red: 0.30, green: 0.90, blue: 0.83),
            ]
        } else {
            palette = [
                .white.opacity(0.055),
                Color(red: 0.31, green: 0.20, blue: 0.48),
                Color(red: 0.48, green: 0.27, blue: 0.68),
                Color(red: 0.67, green: 0.36, blue: 0.82),
                Color(red: 0.86, green: 0.58, blue: 0.98),
            ]
        }
        return palette[min(max(0, level), palette.count - 1)]
    }

    private func weekdayLabel(_ key: String, row: Int, pitch: CGFloat) -> some View {
        Text(localized(key, locale: locale))
            .font(.system(size: 8, weight: .medium))
            .foregroundStyle(.white.opacity(0.28))
            .offset(x: 0, y: 18 + CGFloat(row) * pitch)
    }

    private func monthLabel(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = locale
        formatter.setLocalizedDateFormatFromTemplate("MMM")
        return formatter.string(from: date)
    }

    private func helpText(_ item: TokenDashboardHeatmapCell) -> String {
        let date = TokenDashboardPresentation.heatmapTooltipDate(
            item,
            locale: locale,
            calendar: Calendar.current
        )
        let value = TokenDashboardPresentation.heatmapTooltipValue(
            item,
            metric: metric,
            locale: locale
        )
        return "\(date) · \(value)"
    }

    private func heatmapTooltip(
        _ item: TokenDashboardHeatmapCell,
        width: CGFloat
    ) -> some View {
        HStack(spacing: 6) {
            Text(TokenDashboardPresentation.heatmapTooltipDate(
                item,
                locale: locale,
                calendar: Calendar.current
            ))
            .foregroundStyle(.white.opacity(0.58))
            Text("·")
                .foregroundStyle(.white.opacity(0.28))
            Text(TokenDashboardPresentation.heatmapTooltipValue(
                item,
                metric: metric,
                locale: locale
            ))
            .fontWeight(.bold)
            .foregroundStyle(.white.opacity(0.94))
            .monospacedDigit()
        }
        .font(.system(size: 10.5, weight: .semibold))
        .lineLimit(1)
        .minimumScaleFactor(0.72)
        .padding(.horizontal, 10)
        .frame(width: width, height: 30)
        .background {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(Color(red: 0.045, green: 0.06, blue: 0.08).opacity(0.98))
        }
        .overlay {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(.white.opacity(0.13), lineWidth: 0.75)
        }
        .shadow(color: .black.opacity(0.42), radius: 9, y: 4)
    }
}

private struct TokenDashboardBackground: View {
    var body: some View {
        ZStack {
            Color(red: 0.035, green: 0.045, blue: 0.061)
            LinearGradient(
                colors: [
                    Color(red: 0.055, green: 0.18, blue: 0.28).opacity(0.72),
                    Color(red: 0.035, green: 0.05, blue: 0.07).opacity(0.84),
                    Color.black.opacity(0.24),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            RadialGradient(
                colors: [Color.cyan.opacity(0.10), .clear],
                center: .bottomTrailing,
                startRadius: 20,
                endRadius: 650
            )
        }
        .ignoresSafeArea()
    }
}

private enum TokenDashboardPalette {
    static let series: [Color] = [
        Color(red: 0.25, green: 0.80, blue: 0.90),
        Color(red: 0.95, green: 0.52, blue: 0.30),
        Color(red: 0.57, green: 0.48, blue: 0.95),
        Color(red: 0.38, green: 0.82, blue: 0.55),
        Color(red: 0.96, green: 0.74, blue: 0.28),
        Color(red: 0.86, green: 0.42, blue: 0.68),
        Color(red: 0.38, green: 0.58, blue: 0.94),
        Color(red: 0.68, green: 0.76, blue: 0.35),
    ]
    static let up = Color(red: 0.29, green: 0.83, blue: 0.57)
    static let down = Color(red: 0.96, green: 0.38, blue: 0.40)
    static let warning = Color(red: 0.98, green: 0.68, blue: 0.25)
    static let uncachedInput = Color(red: 0.27, green: 0.66, blue: 0.96)
    static let cacheRead = Color(red: 0.30, green: 0.88, blue: 0.70)
    static let cacheCreation = Color(red: 0.66, green: 0.50, blue: 0.96)
    static let output = Color(red: 0.97, green: 0.57, blue: 0.31)
    static let reasoning = Color(red: 0.91, green: 0.47, blue: 0.79)
}

private extension View {
    func tokenDashboardCard(
        cornerRadius: CGFloat,
        emphasized: Bool = false
    ) -> some View {
        background(
            LinearGradient(
                colors: emphasized
                    ? [.white.opacity(0.09), .black.opacity(0.11)]
                    : [.white.opacity(0.065), .black.opacity(0.09)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            ),
            in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                .strokeBorder(.white.opacity(emphasized ? 0.12 : 0.085), lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.15), radius: 14, y: 6)
    }

    func tokenDashboardAxes(desiredXCount: Int, locale: Locale) -> some View {
        chartYAxis {
            AxisMarks(position: .leading) { value in
                AxisGridLine().foregroundStyle(.white.opacity(0.08))
                AxisTick().foregroundStyle(.clear)
                AxisValueLabel {
                    if let number = value.as(Double.self) {
                        Text(tokenDashboardCompact(UInt64(max(0, number))))
                            .foregroundStyle(.white.opacity(0.35))
                    }
                }
            }
        }
        .chartXAxis {
            AxisMarks(values: .automatic(desiredCount: desiredXCount)) {
                AxisGridLine().foregroundStyle(.clear)
                AxisTick().foregroundStyle(.white.opacity(0.14))
                AxisValueLabel(format: .dateTime.locale(locale).month().day())
                    .foregroundStyle(.white.opacity(0.34))
            }
        }
    }
}

private struct TokenDashboardIconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .bold))
            .foregroundStyle(.white.opacity(0.74))
            .frame(width: 32, height: 32)
            .background(.white.opacity(configuration.isPressed ? 0.13 : 0.065))
            .clipShape(RoundedRectangle(cornerRadius: 9))
            .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(.white.opacity(0.07)))
    }
}

private struct TokenDashboardTabStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 11.5, weight: selected ? .bold : .semibold))
            .foregroundStyle(.white.opacity(selected ? 0.92 : 0.47))
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(selected ? .white.opacity(0.1) : .clear)
            .clipShape(RoundedRectangle(cornerRadius: 9))
            .opacity(configuration.isPressed ? 0.72 : 1)
    }
}

private struct TokenDashboardSegmentStyle: ButtonStyle {
    let selected: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 10, weight: selected ? .bold : .semibold))
            .foregroundStyle(.white.opacity(selected ? 0.9 : 0.4))
            .padding(.horizontal, 11)
            .padding(.vertical, 6)
            .background(selected ? .white.opacity(0.13) : .clear)
            .clipShape(RoundedRectangle(cornerRadius: 7))
            .opacity(configuration.isPressed ? 0.72 : 1)
    }
}

private func tokenDashboardFormattedCost(_ micros: UInt64?, locale: Locale) -> String {
    guard let micros else { return "—" }
    let formatter = NumberFormatter()
    formatter.locale = locale
    formatter.numberStyle = .currency
    formatter.currencyCode = "USD"
    formatter.minimumFractionDigits = 2
    formatter.maximumFractionDigits = micros >= 1_000_000 ? 2 : 4
    return formatter.string(from: NSNumber(value: Double(micros) / 1_000_000)) ?? "—"
}

private func tokenDashboardCompact(_ value: UInt64) -> String {
    let number = Double(value)
    if value >= 1_000_000_000 { return String(format: "%.1fB", number / 1_000_000_000) }
    if value >= 1_000_000 { return String(format: "%.1fM", number / 1_000_000) }
    if value >= 1_000 { return String(format: "%.0fK", number / 1_000) }
    return String(value)
}

private extension UInt64 {
    func saturatingAdd(_ other: UInt64) -> UInt64 {
        addingReportingOverflow(other).overflow ? .max : self + other
    }
}
