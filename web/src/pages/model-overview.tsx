import { TrendLineChart } from "@/components/dashboard-charts";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { StatsCard } from "@/components/stats-card";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useModelMetrics } from "@/hooks/use-model-metrics";
import { type RacePeriod, chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { Coins, DatabaseZap, Gauge, ListChecks, Timer, TrendingUp } from "lucide-react";
import { useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";

/** 三级页三个区块的独立时间段状态。 */
interface ModelOverviewWindows {
	call: RaceWindowState;
	token: RaceWindowState;
	metrics: RaceWindowState;
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

/** 模型详情三级页：调用分析折线 + Token 折线 + 单模型指标卡片，三块独立时间段。 */
export default function ModelOverviewPage() {
	const { providerId: providerIdParam, modelId: modelIdParam } = useParams();
	const providerId = Number.parseInt(providerIdParam ?? "", 10);
	const modelId = decodeURIComponent(modelIdParam ?? "");
	const [searchParams] = useSearchParams();

	// 三块独立时间段，初始值来自 URL（无参数默认当天）。
	const [windows, setWindows] = useState<ModelOverviewWindows>(() => {
		const initial = initialWindowFromUrl(searchParams);
		return { call: { ...initial }, token: { ...initial }, metrics: { ...initial } };
	});
	const [now] = useState(() => Date.now());

	const callWindow = raceWindowBounds(windows.call, now);
	const tokenWindow = raceWindowBounds(windows.token, now);
	const metricsWindow = raceWindowBounds(windows.metrics, now);

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
	const metrics = useModelMetrics(
		Number.isFinite(providerId) ? providerId : -1,
		modelId,
		metricsWindow,
		Number.isFinite(providerId) && modelId.length > 0,
	);

	const windowSubtitle = (state: RaceWindowState) =>
		state.period === "custom"
			? "自定义时间范围"
			: formatPeriodLabel(state.period, state.offset, now);

	const title = `${metrics.data?.providerName || "供应商"}・${modelId} · 模型数据`;

	return (
		<div className="space-y-6">
			<div className="flex items-center gap-2">
				<TrendingUp className="size-5 text-muted-foreground" />
				<h1 className="text-2xl font-bold tracking-tight">{title}</h1>
			</div>

			{/* 调用分析折线（仅折线）：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>调用分析</CardTitle>
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
						<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
							数据加载失败
						</div>
					) : (
						<TrendLineChart
							data={callCharts.data.callTrend}
							label="调用次数"
							granularity={callGranularity}
						/>
					)}
				</CardContent>
			</Card>

			{/* Token 折线（仅折线）：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>Token 分析</CardTitle>
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
						<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
							数据加载失败
						</div>
					) : (
						<TrendLineChart
							data={tokenCharts.data.tokenTrend}
							label="Token 数"
							formatValue={formatTokenCount}
							granularity={tokenGranularity}
						/>
					)}
				</CardContent>
			</Card>

			{/* 单模型指标卡片：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>模型指标</CardTitle>
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
						<div className="flex h-[160px] items-center justify-center text-xs text-muted-foreground">
							数据加载失败
						</div>
					) : (
						<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
							<StatsCard
								icon={Coins}
								label="总计 Token"
								value={formatTokenCount(metrics.data.totalTokens)}
								subLabel="输入 + 输出"
							/>
							<StatsCard
								icon={ListChecks}
								label="请求数"
								value={metrics.data.requestCount.toLocaleString()}
								subLabel="成功请求"
							/>
							<StatsCard
								icon={Timer}
								label="TTFT"
								value={`${metrics.data.ttft.toFixed(1)} ms`}
								subLabel="流式首 token"
							/>
							<StatsCard
								icon={Gauge}
								label="TPS"
								value={metrics.data.tps.toFixed(2)}
								subLabel="加权均值"
							/>
							<StatsCard
								icon={Timer}
								label="平均耗时"
								value={`${metrics.data.requestTime.toFixed(1)} ms`}
								subLabel="成功请求"
							/>
							<StatsCard
								icon={DatabaseZap}
								label="缓存命中率"
								value={formatPercent(metrics.data.cacheHitRate)}
								subLabel="缓存 / 输入 token"
							/>
						</div>
					)}
				</CardContent>
			</Card>
		</div>
	);
}
