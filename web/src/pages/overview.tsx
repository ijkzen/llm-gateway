import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { ProviderModelRaceSection } from "@/components/provider-model-race/ProviderModelRaceSection";
import { ProviderRaceSection } from "@/components/provider-race/ProviderRaceSection";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelRaceSection } from "@/components/virtual-model-race/VirtualModelRaceSection";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts, useDashboardSummary } from "@/hooks/use-dashboard-stats";
import { OVERVIEW_PAGE } from "@/lib/pages";
import { chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { ChartLine, CircleCheck, Coins, DatabaseZap, ListChecks } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

/** 首页调用/Token 分析共享的时间段（默认当天）。 */
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

export default function OverviewPage() {
	const { t } = useTranslation();
	const summaryQuery = useDashboardSummary();
	// 调用/Token 分析共享同一个时间段（默认「今天」）。
	const [chartsWindow, setChartsWindow] = useState<RaceWindowState>(defaultChartsWindow);
	const [now] = useState(() => Date.now());
	const chartsBounds = raceWindowBounds(chartsWindow, now);
	const granularity = chartGranularity(
		chartsWindow.period,
		chartsBounds.startTime,
		chartsBounds.endTime,
	);
	const tzOffsetMinutes = -new Date().getTimezoneOffset();
	const chartsQuery = useDashboardCharts({
		startTime: chartsBounds.startTime,
		endTime: chartsBounds.endTime,
		granularity,
		tzOffsetMinutes,
	});
	const insightQuery = useDashboardInsight({
		startTime: chartsBounds.startTime,
		endTime: chartsBounds.endTime,
		granularity,
		tzOffsetMinutes,
	});

	const isLoading = summaryQuery.isLoading || chartsQuery.isLoading || insightQuery.isLoading;
	const isError = summaryQuery.isError || chartsQuery.isError || insightQuery.isError;

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<StatsCardsSkeleton count={4} />
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

	if (isError || !summaryQuery.data || !chartsQuery.data || !insightQuery.data) {
		return (
			<div className="space-y-6">
				<PageHeader title={t(OVERVIEW_PAGE.titleKey)} icon={ChartLine} />
				<ErrorState
					description={t("overview.errorDescription")}
					onRetry={() => {
						summaryQuery.refetch();
						chartsQuery.refetch();
						insightQuery.refetch();
					}}
				/>
			</div>
		);
	}

	const summary = summaryQuery.data;
	const windowSubtitle =
		chartsWindow.period === "custom"
			? t("overview.customWindow")
			: formatPeriodLabel(chartsWindow.period, chartsWindow.offset, now);

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
					value={formatTokenCount(summary.totalTokens)}
					subLabel={t("overview.inputPlusOutput")}
				/>
				<StatsCard
					icon={DatabaseZap}
					label={t("overview.cacheHitRate")}
					value={formatPercent(summary.cacheHitRate)}
					subLabel={t("overview.cacheTokenOverInput")}
				/>
			</div>

			<div
				className="flex flex-wrap items-center justify-between gap-2"
				data-testid="charts-window"
			>
				<p className="text-xs text-muted-foreground">{windowSubtitle}</p>
				<RaceWindowControl
					state={chartsWindow}
					now={now}
					onChange={(patch) => setChartsWindow((prev) => ({ ...prev, ...patch }))}
				/>
			</div>

			<CallAnalysisCard
				charts={chartsQuery.data}
				subtitle={windowSubtitle}
				granularity={granularity}
			/>
			<TokenAnalysisCard
				charts={chartsQuery.data}
				subtitle={windowSubtitle}
				granularity={granularity}
			/>
			<InsightAnalysisCard
				data={insightQuery.data}
				subtitle={windowSubtitle}
				granularity={granularity}
			/>
			<ProviderRaceSection />
			<VirtualModelRaceSection />
			<ProviderModelRaceSection />
		</div>
	);
}
