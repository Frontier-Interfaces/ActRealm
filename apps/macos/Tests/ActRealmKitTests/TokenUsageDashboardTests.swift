import Foundation
import Testing
@testable import ActRealmKit
@testable import ActRealmUI

@Suite("Token usage dashboard presentation")
struct TokenUsageDashboardTests {
    @Test("final labels require a verified completed ledger")
    func rankedAnalyticsRequireVerifiedLedger() {
        #expect(!TokenDashboardPresentation.analyticsAreFinal(.empty))
        #expect(!TokenDashboardPresentation.analyticsAreFinal(TokenUsageTotals(
            today: 10,
            month: 10,
            total: 10,
            collectionState: "partial",
            dataQuality: "partial"
        )))
        #expect(!TokenDashboardPresentation.analyticsAreFinal(TokenUsageTotals(
            today: 10,
            month: 10,
            total: 10,
            collectionState: "ready",
            dataQuality: "verified",
            collectionInProgress: true
        )))
        #expect(TokenDashboardPresentation.analyticsAreFinal(TokenUsageTotals(
            today: 10,
            month: 10,
            total: 10,
            collectionState: "ready",
            dataQuality: "verified"
        )))
    }

    @Test("one-second app ticks do not invalidate the dashboard render input")
    func dashboardRenderInputChangesOnlyAtDayOrDataBoundaries() throws {
        let calendar = utcCalendar()
        let morning = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 18,
            hour: 9
        )))
        let evening = try #require(calendar.date(byAdding: .hour, value: 10, to: morning))
        let nextDay = try #require(calendar.date(byAdding: .day, value: 1, to: morning))
        let first = TokenDashboardRenderInput(
            tokenUsage: .empty,
            tokenDecision: .empty,
            uiSettings: .defaults,
            now: morning,
            calendar: calendar
        )
        let sameDay = TokenDashboardRenderInput(
            tokenUsage: .empty,
            tokenDecision: .empty,
            uiSettings: .defaults,
            now: evening,
            calendar: calendar
        )
        let followingDay = TokenDashboardRenderInput(
            tokenUsage: .empty,
            tokenDecision: .empty,
            uiSettings: .defaults,
            now: nextDay,
            calendar: calendar
        )

        #expect(first == sameDay)
        #expect(first != followingDay)
    }

    @Test("period selection uses factual provider totals")
    func periodSelectionUsesProviderLedger() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let totals = fixture()

        #expect(TokenDashboardPeriod.day.value(in: totals.byProvider[0]) == 200)
        #expect(TokenDashboardPeriod.month.value(in: totals.byProvider[0]) == 300)
        #expect(TokenDashboardPeriod.total.value(in: totals.byProvider[0]) == 500)
        #expect(TokenDashboardPeriod.day.observedTime(in: totals) == 300)
        #expect(TokenDashboardPeriod.month.observedTime(in: totals) == 1_200)
        #expect(TokenDashboardPeriod.total.observedTime(in: totals) == 3_600)
        #expect(TokenDashboardPeriod.day.executionTime(in: totals) == 120)
        #expect(TokenDashboardPeriod.month.executionTime(in: totals) == 700)
        #expect(TokenDashboardPeriod.total.executionTime(in: totals) == 1_800)
        #expect(
            TokenDashboardPresentation.providerRows(
                totals: totals,
                period: .month,
                now: now,
                calendar: calendar
            ).map(\.total) == [300, 50]
        )
    }

    @Test("missing calendar dates become honest zero-value chart days")
    func chartDaysFillCalendarGaps() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let days = TokenDashboardPresentation.filledDays(
            totals: fixture(),
            count: 3,
            now: now,
            calendar: calendar
        )

        #expect(days.map(\.day) == ["2026-08-12", "2026-08-13", "2026-08-14"])
        #expect(days.map(\.total) == [0, 150, 200])
        #expect(days[0].byProvider.isEmpty)
    }

    @Test("new model detail aggregates only inside the selected period")
    func modelDetailAggregatesBySelectedPeriod() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let rows = TokenDashboardPresentation.modelRows(
            totals: fixture(),
            period: .month,
            now: now,
            calendar: calendar
        )

        #expect(rows.map(\.label) == ["gpt-5.6-sol", "claude-sonnet-5"])
        #expect(rows.map(\.total) == [300, 50])
        #expect(rows[0].inputTokens == 240)
        #expect(rows[0].outputTokens == 60)
        #expect(rows[0].cacheReadTokens == 120)
        #expect(rows[0].reasoningTokens == 15)
        #expect(TokenDashboardPresentation.selectedCost(
            totals: fixture(),
            period: .month,
            now: now,
            calendar: calendar
        ) == 1_750_000)
    }

    @Test("partial pricing is a lower bound with factual coverage")
    func partialPricingShowsLowerBoundCoverage() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let totals = TokenUsageTotals(
            today: 100,
            month: 100,
            total: 100,
            recentDays: [
                TokenUsageDayTotal(
                    day: "2026-08-14",
                    total: 100,
                    estimatedCostUsdMicros: 500_000,
                    pricedTokens: 90,
                    unpricedTokens: 10
                ),
            ]
        )
        let summary = TokenDashboardPresentation.selectedPricingSummary(
            totals: totals,
            period: .day,
            now: now,
            calendar: calendar
        )

        #expect(summary.estimatedCostUsdMicros == 500_000)
        #expect(summary.pricedTokens == 90)
        #expect(summary.unpricedTokens == 10)
        #expect(summary.coverage == 0.9)
        #expect(summary.isPartial)

        let heatmap = TokenDashboardPresentation.heatmapCalendar(
            totals: totals,
            now: now,
            calendar: calendar
        )
        let item = try #require(heatmap.cells.first { $0.day == "2026-08-14" })
        let tooltip = TokenDashboardPresentation.heatmapTooltipValue(
            item,
            metric: .cost,
            locale: Locale(identifier: "zh-Hans")
        )
        #expect(tooltip.contains("至少"))
        #expect(tooltip.contains("90.0%"))
    }

    @Test("provider token semantics normalize into non-overlapping components")
    func tokenCompositionNormalizesCodexAndClaude() throws {
        let composition = TokenDashboardPresentation.composition(rows: [
            TokenDashboardBreakdownRow(
                id: "codex",
                label: "Codex",
                provider: "codex",
                total: 100,
                inputTokens: 80,
                outputTokens: 20,
                cacheReadTokens: 30,
                cacheCreationTokens: nil,
                reasoningTokens: 5,
                estimatedCostUsdMicros: nil
            ),
            TokenDashboardBreakdownRow(
                id: "claude",
                label: "Claude",
                provider: "claude",
                total: 100,
                inputTokens: 10,
                outputTokens: 20,
                cacheReadTokens: 60,
                cacheCreationTokens: 10,
                reasoningTokens: nil,
                estimatedCostUsdMicros: nil
            ),
        ])

        #expect(composition.uncachedInputTokens == 60)
        #expect(composition.cacheReadTokens == 90)
        #expect(composition.cacheCreationTokens == 10)
        #expect(composition.outputTokens == 40)
        #expect(composition.reasoningTokens == 5)
        #expect(composition.unclassifiedTokens == 0)
        #expect(composition.displayTotal == 200)
        #expect(try #require(composition.cacheHitRate) == 0.6)
        #expect(composition.hasInconsistentTotals == false)
    }

    @Test("missing component provenance remains visible as unclassified")
    func missingTokenComponentsRemainHonest() {
        let composition = TokenDashboardPresentation.composition(rows: [
            TokenDashboardBreakdownRow(
                id: "legacy",
                label: "Legacy",
                provider: "codex",
                total: 40,
                inputTokens: nil,
                outputTokens: nil,
                cacheReadTokens: nil,
                cacheCreationTokens: nil,
                reasoningTokens: nil,
                estimatedCostUsdMicros: nil
            ),
        ])

        #expect(composition.hasComponentData == false)
        #expect(composition.unclassifiedTokens == 40)
        #expect(composition.cacheHitRate == nil)
    }

    @Test("legacy snapshots decode with safe dashboard defaults")
    func legacySnapshotDefaultsAreSafe() throws {
        let data = Data("""
        {
          "today": 10,
          "month": 20,
          "total": 30,
          "byProvider": [{"provider":"codex","total":30}],
          "recentDays": [{"day":"2026-08-14","total":10}]
        }
        """.utf8)
        let decoded = try JSONDecoder().decode(TokenUsageTotals.self, from: data)

        #expect(decoded.activeDays == 0)
        #expect(decoded.byProvider[0].today == 0)
        #expect(decoded.byModel.isEmpty)
        #expect(decoded.recentDays[0].byProvider.isEmpty)
        #expect(decoded.messageCount == 0)
        #expect(decoded.recentDays[0].messageCount == 0)
        #expect(decoded.recentDays[0].pricedTokens == 0)
        #expect(decoded.recentDays[0].unpricedTokens == 10)
        #expect(decoded.pricingSources.isEmpty)
        #expect(decoded.executionTimeSeconds == 0)
    }

    @Test("pricing source metadata decodes without exposing session detail")
    func pricingSourceMetadataDecodes() throws {
        let data = Data("""
        {
          "today": 10,
          "month": 20,
          "total": 30,
          "todayExecutionTimeSeconds": 12,
          "monthExecutionTimeSeconds": 70,
          "executionTimeSeconds": 180,
          "pricingSources": [{
            "costKind": "computed",
            "source": "openai_standard_2026-07-20",
            "tokenTotal": 30,
            "estimatedCostUsdMicros": 120
          }],
          "anomalyCount": 1,
          "suspectCount": 1,
          "anomalies": [{
            "code": "future_usage_day",
            "severity": "suspect",
            "scope": "day",
            "day": "2999-01-01",
            "observed": 30
          }]
        }
        """.utf8)
        let decoded = try JSONDecoder().decode(TokenUsageTotals.self, from: data)

        #expect(decoded.pricingSources.count == 1)
        #expect(decoded.pricingSources[0].costKind == "computed")
        #expect(decoded.pricingSources[0].source == "openai_standard_2026-07-20")
        #expect(decoded.pricingSources[0].tokenTotal == 30)
        #expect(decoded.pricingSources[0].estimatedCostUsdMicros == 120)
        #expect(decoded.todayExecutionTimeSeconds == 12)
        #expect(decoded.monthExecutionTimeSeconds == 70)
        #expect(decoded.executionTimeSeconds == 180)
        #expect(decoded.anomalyCount == 1)
        #expect(decoded.suspectCount == 1)
        #expect(decoded.anomalies[0].code == "future_usage_day")
        #expect(decoded.anomalies[0].day == "2999-01-01")
    }

    @Test("heatmap uses Sunday-aligned columns and discrete metric levels")
    func heatmapMatchesContributionCalendarSemantics() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let heatmap = TokenDashboardPresentation.heatmapCalendar(
            totals: fixture(),
            now: now,
            calendar: calendar
        )

        let first = try #require(heatmap.cells.first)
        #expect(first.day == "2025-08-31")
        #expect(first.weekday == 0)
        #expect(first.included == false)
        #expect(heatmap.monthLabels.count == 12)
        #expect(heatmap.monthLabels.first?.column == 0)
        let latest = try #require(heatmap.cells.first { $0.day == "2026-08-14" })
        #expect(latest.weekday == 5)
        #expect(latest.tokenLevel == 4)
        #expect(latest.costLevel == 4)
    }

    @Test("heatmap hover tooltip exposes the exact day and selected metric")
    func heatmapTooltipShowsDayUsage() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let heatmap = TokenDashboardPresentation.heatmapCalendar(
            totals: fixture(),
            now: now,
            calendar: calendar
        )
        let latest = try #require(heatmap.cells.first { $0.day == "2026-08-14" })
        let locale = Locale(identifier: "en_US")

        #expect(TokenDashboardPresentation.heatmapTooltipDate(
            latest,
            locale: locale,
            calendar: calendar
        ) == "Aug 14")
        #expect(TokenDashboardPresentation.heatmapTooltipValue(
            latest,
            metric: .tokens,
            locale: locale
        ) == "Used 200 Tokens")
        #expect(TokenDashboardPresentation.heatmapTooltipValue(
            latest,
            metric: .cost,
            locale: locale
        ) == "Daily API cost estimate $1.00")
    }

    @Test("unknown cost is distinct from a factual zero-cost day")
    func heatmapDoesNotConvertUnknownCostToZero() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let totals = TokenUsageTotals(
            today: 200,
            month: 200,
            total: 200,
            recentDays: [
                TokenUsageDayTotal(day: "2026-08-12", total: 0, estimatedCostUsdMicros: 0),
                TokenUsageDayTotal(
                    day: "2026-08-14",
                    total: 200,
                    estimatedCostUsdMicros: nil
                ),
            ]
        )
        let heatmap = TokenDashboardPresentation.heatmapCalendar(
            totals: totals,
            now: now,
            calendar: calendar
        )
        let active = try #require(heatmap.cells.first { $0.day == "2026-08-14" })
        let empty = try #require(heatmap.cells.first { $0.day == "2026-08-13" })

        #expect(active.costUsdMicros == nil)
        #expect(empty.costUsdMicros == nil)
        #expect(!empty.hasRecord)
        let zero = try #require(heatmap.cells.first { $0.day == "2026-08-12" })
        #expect(zero.hasRecord)
        #expect(zero.costUsdMicros == 0)
        #expect(TokenDashboardPresentation.heatmapTooltipValue(
            active,
            metric: .cost,
            locale: Locale(identifier: "en_US")
        ) == "Cost unavailable")
    }

    @Test("one extreme peak does not flatten ordinary active days")
    func heatmapUsesRobustPeakScaling() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        var records: [TokenUsageDayTotal] = []
        for offset in 0..<100 {
            let date = try #require(calendar.date(byAdding: .day, value: -offset, to: now))
            let value: UInt64 = offset == 0 ? 2_300_000_000 : UInt64(100 - offset)
            records.append(TokenUsageDayTotal(
                day: TokenDashboardPresentation.dayKey(date, calendar: calendar),
                total: value,
                estimatedCostUsdMicros: value
            ))
        }
        let totals = TokenUsageTotals(
            today: 2_300_000_000,
            month: 2_300_000_000,
            total: records.reduce(0) { $0 + $1.total },
            recentDays: records
        )
        let heatmap = TokenDashboardPresentation.heatmapCalendar(
            totals: totals,
            now: now,
            calendar: calendar
        )
        let ordinaryDate = try #require(calendar.date(byAdding: .day, value: -90, to: now))
        let ordinaryKey = TokenDashboardPresentation.dayKey(ordinaryDate, calendar: calendar)
        let ordinary = try #require(heatmap.cells.first { $0.day == ordinaryKey })

        #expect(ordinary.tokens == 10)
        #expect(ordinary.tokenLevel >= 2)
    }

    @Test("stacked legend totals and percentages use visible series")
    func stackedSeriesSummariesAreExact() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let days = TokenDashboardPresentation.filledDays(
            totals: fixture(),
            count: 2,
            now: now,
            calendar: calendar
        )
        let summaries = TokenDashboardPresentation.seriesSummaries(
            points: TokenDashboardPresentation.seriesPoints(
                days: days,
                breakdown: .provider
            )
        )

        #expect(summaries.map(\.series) == ["Codex", "Claude"])
        #expect(summaries.map(\.total) == [300, 50])
        #expect(abs(summaries[0].share - (300.0 / 350.0)) < 0.000_001)
    }

    @Test("chart series merge duplicate model names and omit zero-only marks")
    func chartSeriesAreSafeForDynamicChartScales() {
        let calendar = utcCalendar()
        let day = TokenDashboardDay(
            day: "2026-08-14",
            date: TokenDashboardPresentation.date(
                from: "2026-08-14",
                calendar: calendar
            )!,
            total: 30,
            estimatedCostUsdMicros: nil,
            messageCount: 0,
            byProvider: [],
            byModel: [
                TokenUsageDayModelTotal(provider: "codex", model: "shared", total: 10),
                TokenUsageDayModelTotal(provider: "claude", model: "shared", total: 20),
                TokenUsageDayModelTotal(provider: "codex", model: "Unknown", total: 0),
            ]
        )

        let points = TokenDashboardPresentation.seriesPoints(
            days: [day],
            breakdown: .model
        )
        #expect(points.count == 1)
        #expect(points[0].series == "shared")
        #expect(points[0].value == 30)
    }

    @Test("all range begins at the earliest factual daily record")
    func allRangeUsesEntireArchive() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(
            year: 2026,
            month: 8,
            day: 14,
            hour: 12
        )))
        let days = TokenDashboardPresentation.rangeDays(
            totals: fixture(),
            range: .all,
            now: now,
            calendar: calendar
        )

        #expect(days.map(\.day) == ["2026-08-13", "2026-08-14"])
    }

    @Test("heatmap distinguishes absent history from an observed zero")
    func heatmapDoesNotCallMissingHistoryZero() throws {
        let calendar = utcCalendar()
        let now = try #require(calendar.date(from: DateComponents(year: 2026, month: 8, day: 14, hour: 12)))
        let cells = TokenDashboardPresentation.heatmapCalendar(totals: fixture(), now: now, calendar: calendar).cells
        let missing = try #require(cells.first { $0.day == "2026-08-12" })
        #expect(!missing.hasRecord)
        #expect(missing.costUsdMicros == nil)
        #expect(TokenDashboardPresentation.heatmapTooltipValue(missing, metric: .tokens, locale: Locale(identifier: "en")) == "No observed usage records for this day")
        let recorded = try #require(cells.first { $0.day == "2026-08-14" })
        #expect(recorded.hasRecord)
        #expect(recorded.tokens == 200)
    }

    @Test("usage summary and dashboard share the selected number units")
    func tokenUnitsAreSharedAcrossUsageSurfaces() {
        #expect(TokenDashboardPresentation.tokenText(9_000_000_000, unitStyle: .western, locale: Locale(identifier: "en")) == "9.00B")
        #expect(TokenDashboardPresentation.tokenText(1_250_000, unitStyle: .automatic, locale: Locale(identifier: "zh-Hans")) == "125万")
        #expect(TokenDashboardPresentation.tokenText(0, unitStyle: .automatic, locale: Locale(identifier: "en")) == "0")
    }

    private func fixture() -> TokenUsageTotals {
        TokenUsageTotals(
            today: 200,
            month: 350,
            total: 550,
            activeDays: 2,
            currentStreak: 2,
            todayActiveTimeSeconds: 300,
            monthActiveTimeSeconds: 1_200,
            activeTimeSeconds: 3_600,
            todayExecutionTimeSeconds: 120,
            monthExecutionTimeSeconds: 700,
            executionTimeSeconds: 1_800,
            turnCount: 8,
            messageCount: 13,
            peakDay: "2026-08-14",
            peakDayTotal: 200,
            byProvider: [
                TokenUsageProviderTotal(
                    provider: "codex",
                    total: 500,
                    today: 200,
                    month: 300
                ),
                TokenUsageProviderTotal(
                    provider: "claude",
                    total: 50,
                    today: 0,
                    month: 50
                ),
            ],
            byModel: [
                TokenUsageModelTotal(
                    provider: "codex",
                    model: "gpt-5.6-sol",
                    total: 500,
                    inputTokens: 450,
                    outputTokens: 50,
                    cacheReadTokens: 300,
                    cacheCreationTokens: nil,
                    reasoningTokens: 20,
                    estimatedCostUsdMicros: 2_500_000
                ),
                TokenUsageModelTotal(
                    provider: "claude",
                    model: "claude-sonnet-5",
                    total: 50,
                    inputTokens: 40,
                    outputTokens: 10,
                    cacheReadTokens: 20,
                    cacheCreationTokens: 5,
                    reasoningTokens: nil,
                    estimatedCostUsdMicros: 250_000
                ),
            ],
            recentDays: [
                TokenUsageDayTotal(
                    day: "2026-08-13",
                    total: 150,
                    estimatedCostUsdMicros: 750_000,
                    messageCount: 5,
                    byProvider: [
                        TokenUsageDayProviderTotal(
                            provider: "codex",
                            total: 100,
                            estimatedCostUsdMicros: 500_000,
                            messageCount: 3
                        ),
                        TokenUsageDayProviderTotal(
                            provider: "claude",
                            total: 50,
                            estimatedCostUsdMicros: 250_000,
                            messageCount: 2
                        ),
                    ],
                    byModel: [
                        TokenUsageDayModelTotal(
                            provider: "codex",
                            model: "gpt-5.6-sol",
                            total: 100,
                            inputTokens: 80,
                            outputTokens: 20,
                            cacheReadTokens: 40,
                            reasoningTokens: 5,
                            estimatedCostUsdMicros: 500_000
                        ),
                        TokenUsageDayModelTotal(
                            provider: "claude",
                            model: "claude-sonnet-5",
                            total: 50,
                            inputTokens: 40,
                            outputTokens: 10,
                            cacheReadTokens: 20,
                            cacheCreationTokens: 5,
                            estimatedCostUsdMicros: 250_000
                        ),
                    ]
                ),
                TokenUsageDayTotal(
                    day: "2026-08-14",
                    total: 200,
                    estimatedCostUsdMicros: 1_000_000,
                    messageCount: 8,
                    byProvider: [
                        TokenUsageDayProviderTotal(
                            provider: "codex",
                            total: 200,
                            estimatedCostUsdMicros: 1_000_000,
                            messageCount: 8
                        ),
                    ],
                    byModel: [
                        TokenUsageDayModelTotal(
                            provider: "codex",
                            model: "gpt-5.6-sol",
                            total: 200,
                            inputTokens: 160,
                            outputTokens: 40,
                            cacheReadTokens: 80,
                            reasoningTokens: 10,
                            estimatedCostUsdMicros: 1_000_000
                        ),
                    ]
                ),
            ],
            detailRecordedFrom: 1
        )
    }

    private func dashboardDay(
        _ day: String,
        total: UInt64,
        calendar: Calendar
    ) -> TokenDashboardDay {
        TokenDashboardDay(
            day: day,
            date: TokenDashboardPresentation.date(from: day, calendar: calendar)!,
            total: total,
            estimatedCostUsdMicros: nil,
            messageCount: 0,
            byProvider: [],
            byModel: []
        )
    }

    private func utcCalendar() -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(secondsFromGMT: 0)!
        calendar.locale = Locale(identifier: "en_US_POSIX")
        return calendar
    }
}
