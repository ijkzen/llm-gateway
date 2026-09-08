import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { ProviderModelRaceCard } from "@/components/provider-model-race/ProviderModelRaceCard";
import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import type { RaceWindowState } from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import {
	StatsSection,
	sectionGranularity,
	sectionWindow,
	useSectionSubtitle,
	useSectionWindows,
} from "@/components/stats-section";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelRaceCard } from "@/components/virtual-model-race/VirtualModelRaceCard";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts, useDashboardSummary } from "@/hooks/use-dashboard-stats";
import { useStatsTimeZone } from "@/hooks/use-stats-time-zone";
import { OVERVIEW_PAGE } from "@/lib/pages";
import { periodBounds } from "@/lib/race-period";
import { formatPercent, formatTokenCount, localeOf } from "@/lib/utils";
import { ChartLine, CircleCheck, Coins, DatabaseZap, ListChecks } from "lucide-react";
import { useTranslation } from "react-i18next";

/** 首页调用/Token/可靠性分析共享的初始时间段（默认当天，本地 0 点起）。 */
function defaultChartsWindow(): RaceWindowState {
	const now = Date.now();
	const start = new Date(now);
	start.setHours(0, 0, 0, 0);
	return {
		period: "day",
		offset: 0,
		customStart: start.getTime(),
		customEnd: now,
		appliedCustom: null,
	};
}

const SECTION_KEYS = ["call", "token", "insight"] as const;

export default function OverviewPage() {
	const { t, i18n } = useTranslation();
	// 今日窗口：设置表时区今日 0 点 → 当前时刻（与图表区「天」周期同一语义）。
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS, defaultChartsWindow);
	const subtitle = useSectionSubtitle();
	const tz = useStatsTimeZone();
	const todayWindow = periodBounds("day", 0, now, tz);
	const summaryQuery = useDashboardSummary();
	const todaySummaryQuery = useDashboardSummary({
		startTime: todayWindow.startTime,
		endTime: todayWindow.endTime,
	});
	// 调用/Token/可靠性分析各自独立时间段（默认「今天」）。
	const callWindow = sectionWindow(windows.call, now, tz);
	const tokenWindow = sectionWindow(windows.token, now, tz);
	const insightWindow = sectionWindow(windows.insight, now, tz);
	const callGranularity = sectionGranularity(windows.call, callWindow);
	const tokenGranularity = sectionGranularity(windows.token, tokenWindow);
	const insightGranularity = sectionGranularity(windows.insight, insightWindow);
	const callChartsQuery = useDashboardCharts({
		startTime: callWindow.startTime,
		endTime: callWindow.endTime,
		granularity: callGranularity,
	});
	const tokenChartsQuery = useDashboardCharts({
		startTime: tokenWindow.startTime,
		endTime: tokenWindow.endTime,
		granularity: tokenGranularity,
	});
	const insightQuery = useDashboardInsight({
		startTime: insightWindow.startTime,
		endTime: insightWindow.endTime,
		granularity: insightGranularity,
	});

	const isLoading =
		summaryQuery.isLoading ||
		todaySummaryQuery.isLoading ||
		callChartsQuery.isLoading ||
		tokenChartsQuery.isLoading ||
		insightQuery.isLoading;
	const isError =
		summaryQuery.isError ||
		todaySummaryQuery.isError ||
		callChartsQuery.isError ||
		tokenChartsQuery.isError ||
		insightQuery.isError;

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<StatsCardsSkeleton count={8} />
				<Card>
					<CardHeader>
						<Skeleton className="h-5 w-24" />
					</CardHeader>
					<CardContent>
						<Skeleton className="h-[260px] w-full" />
					</CardContent>
				</Card>
			</div>
		);
	}

	if (
		isError ||
		!summaryQuery.data ||
		!todaySummaryQuery.data ||
		!callChartsQuery.data ||
		!tokenChartsQuery.data ||
		!insightQuery.data
	) {
		return (
			<div className="space-y-6">
				<PageHeader title={t(OVERVIEW_PAGE.titleKey)} icon={ChartLine} />
				<ErrorState
					description={t("overview.errorDescription")}
					onRetry={() => {
						summaryQuery.refetch();
						todaySummaryQuery.refetch();
						callChartsQuery.refetch();
						tokenChartsQuery.refetch();
						insightQuery.refetch();
					}}
				/>
			</div>
		);
	}

	const summary = summaryQuery.data;
	const todaySummary = todaySummaryQuery.data;

	return (
		<div className="space-y-6">
			<PageHeader title={t(OVERVIEW_PAGE.titleKey)} icon={ChartLine} />

			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
				<StatsCard
					icon={ListChecks}
					label={t("overview.totalRequests")}
					value={summary.totalRequests.toLocaleString()}
					subLabel={t("overview.allHistory")}
				/>
				<StatsCard
					icon={CircleCheck}
					label={t("overview.successRate")}
					value={formatPercent(summary.successRate)}
					subLabel={t("overview.allHistory")}
				/>
				<StatsCard
					icon={Coins}
					label={t("overview.totalTokens")}
					value={formatTokenCount(summary.totalTokens, localeOf(i18n.language))}
					subLabel={t("overview.inputPlusOutput")}
				/>
				<StatsCard
					icon={DatabaseZap}
					label={t("overview.cacheHitRate")}
					value={formatPercent(summary.cacheHitRate)}
					subLabel={t("overview.cacheTokenOverInput")}
				/>
			</div>

			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
				<StatsCard
					icon={ListChecks}
					label={t("overview.todayRequests")}
					value={todaySummary.totalRequests.toLocaleString()}
					subLabel={t("overview.today")}
				/>
				<StatsCard
					icon={CircleCheck}
					label={t("overview.successRate")}
					value={formatPercent(todaySummary.successRate)}
					subLabel={t("overview.today")}
				/>
				<StatsCard
					icon={Coins}
					label={t("overview.totalTokens")}
					value={formatTokenCount(todaySummary.totalTokens, localeOf(i18n.language))}
					subLabel={t("overview.today")}
				/>
				<StatsCard
					icon={DatabaseZap}
					label={t("overview.cacheHitRate")}
					value={formatPercent(todaySummary.cacheHitRate)}
					subLabel={t("overview.today")}
				/>
			</div>

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳；页面级统一门控） */}
			<StatsSection
				now={now}
				windowState={windows.call}
				onWindowChange={setWindow("call")}
				windowTestId="call-window"
			>
				<CallAnalysisCard
					charts={callChartsQuery.data}
					subtitle={subtitle(windows.call, now)}
					granularity={callGranularity}
				/>
			</StatsSection>

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳；页面级统一门控） */}
			<StatsSection
				now={now}
				windowState={windows.token}
				onWindowChange={setWindow("token")}
				windowTestId="token-window"
			>
				<TokenAnalysisCard
					charts={tokenChartsQuery.data}
					subtitle={subtitle(windows.token, now)}
					granularity={tokenGranularity}
				/>
			</StatsSection>

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳；页面级统一门控） */}
			<StatsSection
				now={now}
				windowState={windows.insight}
				onWindowChange={setWindow("insight")}
				windowTestId="insight-window"
			>
				<InsightAnalysisCard
					data={insightQuery.data}
					subtitle={subtitle(windows.insight, now)}
					granularity={insightGranularity}
				/>
			</StatsSection>
			<ApiKeyRaceCard />
			<ProviderRaceCard />
			<VirtualModelRaceCard />
			<ProviderModelRaceCard />
		</div>
	);
}
