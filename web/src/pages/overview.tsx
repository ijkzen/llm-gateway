import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { ProviderModelRaceCard } from "@/components/provider-model-race/ProviderModelRaceCard";
import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelRaceCard } from "@/components/virtual-model-race/VirtualModelRaceCard";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts, useDashboardSummary } from "@/hooks/use-dashboard-stats";
import { OVERVIEW_PAGE } from "@/lib/pages";
import { chartGranularity, formatPeriodLabel, periodBounds } from "@/lib/race-period";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { ChartLine, CircleCheck, Coins, DatabaseZap, ListChecks } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

/** 首页调用/Token/可靠性分析共享的初始时间段（默认当天）。 */
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

/** 首页三块分析图的独立时间段。 */
interface OverviewWindows {
	call: RaceWindowState;
	token: RaceWindowState;
	insight: RaceWindowState;
}

function initialWindows(): OverviewWindows {
	const initial = defaultChartsWindow();
	return { call: { ...initial }, token: { ...initial }, insight: { ...initial } };
}

export default function OverviewPage() {
	const { t } = useTranslation();
	const [now] = useState(() => Date.now());
	// 今日窗口：本地今日 0 点 → 当前时刻（与图表区「天」周期同一语义）。
	const todayWindow = periodBounds("day", 0, now);
	const summaryQuery = useDashboardSummary();
	const todaySummaryQuery = useDashboardSummary({
		startTime: todayWindow.startTime,
		endTime: todayWindow.endTime,
	});
	// 调用/Token/可靠性分析各自独立时间段（默认「今天」）。
	const [windows, setWindows] = useState<OverviewWindows>(initialWindows);
	const callWindow = raceWindowBounds(windows.call, now);
	const tokenWindow = raceWindowBounds(windows.token, now);
	const insightWindow = raceWindowBounds(windows.insight, now);
	const callGranularity = chartGranularity(
		windows.call.period,
		callWindow.startTime,
		callWindow.endTime,
	);
	const tokenGranularity = chartGranularity(
		windows.token.period,
		tokenWindow.startTime,
		tokenWindow.endTime,
	);
	const insightGranularity = chartGranularity(
		windows.insight.period,
		insightWindow.startTime,
		insightWindow.endTime,
	);
	const tzOffsetMinutes = -new Date().getTimezoneOffset();
	const callChartsQuery = useDashboardCharts({
		startTime: callWindow.startTime,
		endTime: callWindow.endTime,
		granularity: callGranularity,
		tzOffsetMinutes,
	});
	const tokenChartsQuery = useDashboardCharts({
		startTime: tokenWindow.startTime,
		endTime: tokenWindow.endTime,
		granularity: tokenGranularity,
		tzOffsetMinutes,
	});
	const insightQuery = useDashboardInsight({
		startTime: insightWindow.startTime,
		endTime: insightWindow.endTime,
		granularity: insightGranularity,
		tzOffsetMinutes,
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
	const windowSubtitle = (windowState: RaceWindowState) =>
		windowState.period === "custom"
			? t("overview.customWindow")
			: formatPeriodLabel(windowState.period, windowState.offset, now);
	const setWindow = (key: keyof OverviewWindows) => (patch: Partial<RaceWindowState>) =>
		setWindows((prev) => ({ ...prev, [key]: { ...prev[key], ...patch } }));

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
					value={formatTokenCount(todaySummary.totalTokens)}
					subLabel={t("overview.today")}
				/>
				<StatsCard
					icon={DatabaseZap}
					label={t("overview.cacheHitRate")}
					value={formatPercent(todaySummary.cacheHitRate)}
					subLabel={t("overview.today")}
				/>
			</div>

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div
					className="flex flex-wrap items-center justify-between gap-2"
					data-testid="call-window"
				>
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.call)}</p>
					<RaceWindowControl state={windows.call} now={now} onChange={setWindow("call")} />
				</div>
				<CallAnalysisCard
					charts={callChartsQuery.data}
					subtitle={windowSubtitle(windows.call)}
					granularity={callGranularity}
				/>
			</div>

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div
					className="flex flex-wrap items-center justify-between gap-2"
					data-testid="token-window"
				>
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.token)}</p>
					<RaceWindowControl state={windows.token} now={now} onChange={setWindow("token")} />
				</div>
				<TokenAnalysisCard
					charts={tokenChartsQuery.data}
					subtitle={windowSubtitle(windows.token)}
					granularity={tokenGranularity}
				/>
			</div>

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div
					className="flex flex-wrap items-center justify-between gap-2"
					data-testid="insight-window"
				>
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.insight)}</p>
					<RaceWindowControl state={windows.insight} now={now} onChange={setWindow("insight")} />
				</div>
				<InsightAnalysisCard
					data={insightQuery.data}
					subtitle={windowSubtitle(windows.insight)}
					granularity={insightGranularity}
				/>
			</div>
			<ApiKeyRaceCard />
			<ProviderRaceCard />
			<VirtualModelRaceCard />
			<ProviderModelRaceCard />
		</div>
	);
}
