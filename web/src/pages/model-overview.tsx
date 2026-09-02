import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { TrendLineChart } from "@/components/dashboard-charts";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useModelMetrics } from "@/hooks/use-model-metrics";
import { type RacePeriod, chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { Coins, DatabaseZap, Gauge, ListChecks, Timer, TrendingUp } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useSearchParams } from "react-router-dom";

/** 三级页四个区块的独立时间段状态。 */
interface ModelOverviewWindows {
	call: RaceWindowState;
	token: RaceWindowState;
	metrics: RaceWindowState;
	insight: RaceWindowState;
}

/** 从 URL query 解析初始时间段（缺省当天）；入口赛马行点击时携带。 */
function initialWindowFromUrl(searchParams: URLSearchParams): RaceWindowState {
	const period = (searchParams.get("period") as RacePeriod | "custom" | null) ?? "day";
	const offset = Number.parseInt(searchParams.get("offset") ?? "0", 10) || 0;
	const now = Date.now();
	const startTime = Number(searchParams.get("startTime")) || now - 3_600_000;
	const endTime = Number(searchParams.get("endTime")) || now;
	return {
		period,
		offset,
		customStart: startTime,
		customEnd: endTime,
		appliedCustom: period === "custom" ? { startTime, endTime } : null,
	};
}

/** 模型详情三级页：单模型指标卡片（置顶）+ 调用分析折线 + Token 折线，三块独立时间段。 */
export default function ModelOverviewPage() {
	const { t } = useTranslation();
	const { providerId: providerIdParam, modelId: modelIdParam } = useParams();
	const providerId = Number.parseInt(providerIdParam ?? "", 10);
	const modelId = decodeURIComponent(modelIdParam ?? "");
	const [searchParams] = useSearchParams();

	// 五块独立时间段，初始值来自 URL（无参数默认当天）。
	const [windows, setWindows] = useState<ModelOverviewWindows>(() => {
		const initial = initialWindowFromUrl(searchParams);
		return {
			call: { ...initial },
			token: { ...initial },
			metrics: { ...initial },
			insight: { ...initial },
		};
	});
	const [now] = useState(() => Date.now());

	const callWindow = raceWindowBounds(windows.call, now);
	const tokenWindow = raceWindowBounds(windows.token, now);
	const metricsWindow = raceWindowBounds(windows.metrics, now);
	const insightWindow = raceWindowBounds(windows.insight, now);

	// 图表桶粒度由所选时间窗口推导，并与本地时区偏移一起传给后端。
	const tzOffsetMinutes = -new Date().getTimezoneOffset();
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

	const callCharts = useDashboardCharts({
		startTime: callWindow.startTime,
		endTime: callWindow.endTime,
		providerId,
		modelId,
		granularity: callGranularity,
		tzOffsetMinutes,
	});
	const tokenCharts = useDashboardCharts({
		startTime: tokenWindow.startTime,
		endTime: tokenWindow.endTime,
		providerId,
		modelId,
		granularity: tokenGranularity,
		tzOffsetMinutes,
	});
	const insightQuery = useDashboardInsight({
		startTime: insightWindow.startTime,
		endTime: insightWindow.endTime,
		providerId,
		modelId,
		granularity: insightGranularity,
		tzOffsetMinutes,
	});
	const metrics = useModelMetrics(
		Number.isFinite(providerId) ? providerId : -1,
		modelId,
		metricsWindow,
		Number.isFinite(providerId) && modelId.length > 0,
	);

	const windowSubtitle = (state: RaceWindowState) =>
		state.period === "custom"
			? t("overview.customWindow")
			: formatPeriodLabel(state.period, state.offset, now);

	const title = `${metrics.data?.providerName || t("dashboardPage.providerFallback")}・${modelId} · ${t("dashboardPage.modelTitleSuffix")}`;

	return (
		<div className="space-y-6">
			<PageHeader icon={TrendingUp} title={title} />

			{/* 单模型指标卡片：独立时间段（置顶，概览优先） */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>{t("dashboard.modelMetric")}</CardTitle>
						<p className="text-xs text-muted-foreground">{windowSubtitle(windows.metrics)}</p>
					</div>
					<RaceWindowControl
						state={windows.metrics}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, metrics: { ...prev.metrics, ...patch } }))
						}
					/>
				</CardHeader>
				<CardContent>
					{metrics.isLoading ? (
						<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
							{[0, 1, 2, 3, 4, 5].map((i) => (
								<Skeleton key={i} className="h-24 w-full" />
							))}
						</div>
					) : metrics.isError || !metrics.data ? (
						<ErrorState onRetry={() => metrics.refetch()} />
					) : (
						<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
							<StatsCard
								icon={Coins}
								label={t("race.totalTokens")}
								value={formatTokenCount(metrics.data.totalTokens)}
								subLabel={t("overview.inputPlusOutput")}
							/>
							<StatsCard
								icon={ListChecks}
								label={t("race.requests")}
								value={metrics.data.requestCount.toLocaleString()}
								subLabel={t("overview.successRequests")}
							/>
							<StatsCard
								icon={Timer}
								label={t("race.ttft")}
								value={`${metrics.data.ttft.toFixed(1)} ms`}
								subLabel={t("overview.streamFirstToken")}
							/>
							<StatsCard
								icon={Gauge}
								label={t("race.tps")}
								value={metrics.data.tps.toFixed(2)}
								subLabel={t("overview.weightedAverage")}
							/>
							<StatsCard
								icon={Timer}
								label={t("race.avgLatency")}
								value={`${metrics.data.requestTime.toFixed(1)} ms`}
								subLabel={t("overview.successRequests")}
							/>
							<StatsCard
								icon={DatabaseZap}
								label={t("race.cacheHitRate")}
								value={formatPercent(metrics.data.cacheHitRate)}
								subLabel={t("overview.cacheOverInputToken")}
							/>
						</div>
					)}
				</CardContent>
			</Card>

			{/* 调用分析折线（仅折线）：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>{t("dashboard.analysis")}</CardTitle>
						<p className="text-xs text-muted-foreground">{windowSubtitle(windows.call)}</p>
					</div>
					<RaceWindowControl
						state={windows.call}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, call: { ...prev.call, ...patch } }))
						}
					/>
				</CardHeader>
				<CardContent>
					{callCharts.isLoading ? (
						<Skeleton className="h-[260px] w-full" />
					) : callCharts.isError || !callCharts.data ? (
						<ErrorState onRetry={() => callCharts.refetch()} />
					) : (
						<TrendLineChart
							data={callCharts.data.callTrend}
							label={t("overview.calls")}
							granularity={callGranularity}
						/>
					)}
				</CardContent>
			</Card>

			{/* Token 折线（仅折线）：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>{t("dashboard.tokenAnalysis")}</CardTitle>
						<p className="text-xs text-muted-foreground">{windowSubtitle(windows.token)}</p>
					</div>
					<RaceWindowControl
						state={windows.token}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, token: { ...prev.token, ...patch } }))
						}
					/>
				</CardHeader>
				<CardContent>
					{tokenCharts.isLoading ? (
						<Skeleton className="h-[260px] w-full" />
					) : tokenCharts.isError || !tokenCharts.data ? (
						<ErrorState onRetry={() => tokenCharts.refetch()} />
					) : (
						<TrendLineChart
							data={tokenCharts.data.tokenTrend}
							label={t("overview.tokens")}
							formatValue={formatTokenCount}
							kind="tokens"
							granularity={tokenGranularity}
						/>
					)}
				</CardContent>
			</Card>

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div className="flex flex-wrap items-center justify-between gap-2">
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.insight)}</p>
					<RaceWindowControl
						state={windows.insight}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, insight: { ...prev.insight, ...patch } }))
						}
					/>
				</div>
				{insightQuery.isLoading ? (
					<Skeleton className="h-[260px] w-full" />
				) : insightQuery.isError || !insightQuery.data ? (
					<ErrorState onRetry={() => insightQuery.refetch()} />
				) : (
					<InsightAnalysisCard
						data={insightQuery.data}
						subtitle={windowSubtitle(windows.insight)}
						granularity={insightGranularity}
					/>
				)}
			</div>

			{/* API Key 赛马：独立时间段（按当前供应商+模型过滤） */}
			<ApiKeyRaceCard
				filter={
					Number.isFinite(providerId) && modelId.length > 0 ? { providerId, modelId } : undefined
				}
			/>
		</div>
	);
}
