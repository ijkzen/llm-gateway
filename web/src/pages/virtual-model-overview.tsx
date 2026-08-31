import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useVirtualModelMetrics } from "@/hooks/use-stats-metrics";
import {
	type RaceSort,
	type RaceSortKey,
	type VirtualModelMemberRankItem,
	useVirtualModelMemberRank,
} from "@/hooks/use-virtual-model-member-rank";
import { useVirtualModelDetail } from "@/hooks/use-virtual-models";
import { type RacePeriod, chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { ArrowDown, ArrowUp, Boxes } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

/** 二级页五个图表区块的独立时间段状态。 */
interface VirtualModelOverviewWindows {
	metrics: RaceWindowState;
	call: RaceWindowState;
	token: RaceWindowState;
	race: RaceWindowState;
	insight: RaceWindowState;
}

/** 从 URL query 解析初始时间段（缺省当天）；首页赛马行点击时携带。 */
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

/** 6 列指标定义（成员模型赛马表格）。 */
const METRIC_KEYS: Record<RaceSortKey, string> = {
	totalTokens: "race.metricLabel.totalTokens",
	requestCount: "race.metricLabel.requestCount",
	ttft: "race.metricLabel.ttft",
	requestTime: "race.metricLabel.requestTime",
	tps: "race.metricLabel.tps",
	cacheHitRate: "race.metricLabel.cacheHitRate",
};

const COLUMNS: ReadonlyArray<{
	key: RaceSortKey;
	format: (v: number) => string;
	defaultDesc: boolean;
}> = [
	{ key: "totalTokens", format: formatTokenCount, defaultDesc: true },
	{ key: "requestCount", format: (v) => v.toLocaleString(), defaultDesc: true },
	{ key: "ttft", format: (v) => `${v.toFixed(1)} ms`, defaultDesc: false },
	{ key: "requestTime", format: (v) => `${v.toFixed(1)} ms`, defaultDesc: false },
	{ key: "tps", format: (v) => v.toFixed(2), defaultDesc: true },
	{ key: "cacheHitRate", format: formatPercent, defaultDesc: true },
];

/** 成员模型赛马表格（配置成员全量 + 6 指标 + 排序；停用成员灰显）。 */
function MemberModelRaceTable({
	virtualModelId,
	windowState,
	now,
}: {
	virtualModelId: number;
	windowState: RaceWindowState;
	now: number;
}) {
	const navigate = useNavigate();
	const { t } = useTranslation();
	const [sort, setSort] = useState<RaceSort>({ sortBy: "totalTokens", sortOrder: "desc" });
	const window = raceWindowBounds(windowState, now);
	const query = useVirtualModelMemberRank(window, sort, true, virtualModelId);

	const handleSort = (key: RaceSortKey) => {
		setSort((prev) => {
			if (prev.sortBy === key) {
				return { ...prev, sortOrder: prev.sortOrder === "asc" ? "desc" : "asc" };
			}
			const column = COLUMNS.find((c) => c.key === key);
			return { sortBy: key, sortOrder: column?.defaultDesc ? "desc" : "asc" };
		});
	};

	const openModelOverview = (item: VirtualModelMemberRankItem) => {
		const params = new URLSearchParams();
		if (windowState.period === "custom") {
			params.set("period", "custom");
			params.set("startTime", String(window.startTime));
			params.set("endTime", String(window.endTime));
		} else {
			params.set("period", windowState.period);
			params.set("offset", String(windowState.offset));
		}
		navigate(
			`/models/${item.providerId}/${encodeURIComponent(item.modelId)}/overview?${params.toString()}`,
		);
	};

	if (query.isLoading) {
		return <Skeleton className="h-[220px] w-full" />;
	}
	if (query.isError || !query.data) {
		return (
			<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
				{t("overview.dataLoadFailed")}
			</div>
		);
	}

	return (
		<div className="overflow-x-auto">
			<table className="w-full min-w-[720px] border-collapse text-sm">
				<thead>
					<tr className="border-b border-foreground/10">
						<th className="w-10 px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							#
						</th>
						<th className="px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							{t("dashboard.providerModel")}
						</th>
						{COLUMNS.map((column) => {
							const active = sort.sortBy === column.key;
							const label = t(METRIC_KEYS[column.key]);
							return (
								<th key={column.key} className="px-2 py-2 text-right">
									<button
										type="button"
										onClick={() => handleSort(column.key)}
										aria-label={label}
										className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium transition-colors hover:bg-foreground/5 ${
											active ? "text-foreground" : "text-muted-foreground"
										}`}
									>
										{label}
										{active &&
											(sort.sortOrder === "asc" ? (
												<ArrowUp data-testid={`sort-${column.key}`} className="h-3 w-3" />
											) : (
												<ArrowDown data-testid={`sort-${column.key}`} className="h-3 w-3" />
											))}
									</button>
								</th>
							);
						})}
					</tr>
				</thead>
				<tbody>
					{query.data.items.map((item: VirtualModelMemberRankItem, index: number) => (
						<tr
							key={`${item.providerName}::${item.modelId}`}
							onClick={() => openModelOverview(item)}
							onKeyDown={(e) => {
								if (e.key === "Enter") {
									openModelOverview(item);
								}
							}}
							tabIndex={0}
							title={t("race.openModelDetail")}
							className={`cursor-pointer border-b border-foreground/5 last:border-0 hover:bg-foreground/5 ${
								item.memberEnable ? "" : "opacity-50"
							}`}
						>
							<td className="px-2 py-2 text-left font-mono text-xs text-muted-foreground">
								{index + 1}
							</td>
							<td className="px-2 py-2 text-left font-medium text-foreground">
								{item.providerName || t("race.unknownProvider")}・{item.modelId}
								{!item.memberEnable && (
									<span className="ml-2 text-xs text-muted-foreground">
										{t("race.disabledSuffix")}
									</span>
								)}
							</td>
							{COLUMNS.map((column) => (
								<td
									key={column.key}
									className="px-2 py-2 text-right font-mono text-xs tabular-nums text-foreground"
								>
									{column.format(item[column.key])}
								</td>
							))}
						</tr>
					))}
				</tbody>
			</table>
		</div>
	);
}

/** 虚拟模型二级数据面板：调用分析 + token 分析 + 成员模型赛马，三块独立时间段。 */
export default function VirtualModelOverviewPage() {
	const { t } = useTranslation();
	const { virtualModelId: virtualModelIdParam } = useParams();
	const virtualModelId = Number.parseInt(virtualModelIdParam ?? "", 10);
	const [searchParams] = useSearchParams();

	// 四块独立时间段，初始值来自 URL（无参数默认当天）。
	const [windows, setWindows] = useState<VirtualModelOverviewWindows>(() => {
		const initial = initialWindowFromUrl(searchParams);
		return {
			metrics: { ...initial },
			call: { ...initial },
			token: { ...initial },
			race: { ...initial },
			insight: { ...initial },
		};
	});
	// 各块固化 now（标题稳定）。
	const [now] = useState(() => Date.now());

	const detail = useVirtualModelDetail(Number.isFinite(virtualModelId) ? virtualModelId : null);
	const displayId =
		detail.data?.displayId ?? t("dashboardPage.virtualModelLabel", { id: virtualModelId });

	const metricsWindow = raceWindowBounds(windows.metrics, now);
	const callWindow = raceWindowBounds(windows.call, now);
	const tokenWindow = raceWindowBounds(windows.token, now);
	const insightWindow = raceWindowBounds(windows.insight, now);

	const vmMetrics = useVirtualModelMetrics(
		virtualModelId,
		metricsWindow,
		Number.isFinite(virtualModelId),
	);

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
		virtualModelId,
		granularity: callGranularity,
		tzOffsetMinutes,
	});
	const tokenCharts = useDashboardCharts({
		startTime: tokenWindow.startTime,
		endTime: tokenWindow.endTime,
		virtualModelId,
		granularity: tokenGranularity,
		tzOffsetMinutes,
	});
	const insightQuery = useDashboardInsight({
		startTime: insightWindow.startTime,
		endTime: insightWindow.endTime,
		virtualModelId,
		granularity: insightGranularity,
		tzOffsetMinutes,
	});

	const windowSubtitle = (state: RaceWindowState) =>
		state.period === "custom"
			? t("overview.customWindow")
			: formatPeriodLabel(state.period, state.offset, now);

	return (
		<div className="space-y-6">
			<div className="flex items-center gap-2">
				<Boxes className="size-5 text-muted-foreground" />
				<h1 className="text-2xl font-bold tracking-tight">
					{displayId} · {t("dashboardPage.titleSuffix")}
				</h1>
			</div>

			{/* 顶部：6 指标概览（独立时间段；虚拟模型无用量信息） */}
			<MetricsSummaryCard
				data={vmMetrics.data}
				isLoading={vmMetrics.isLoading}
				windowState={windows.metrics}
				now={now}
				onWindowChange={(patch) =>
					setWindows((prev) => ({ ...prev, metrics: { ...prev.metrics, ...patch } }))
				}
				subtitle={windowSubtitle(windows.metrics)}
			/>

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div className="flex flex-wrap items-center justify-between gap-2">
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.call)}</p>
					<RaceWindowControl
						state={windows.call}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, call: { ...prev.call, ...patch } }))
						}
					/>
				</div>
				{callCharts.isLoading ? (
					<Skeleton className="h-[260px] w-full" />
				) : callCharts.isError || !callCharts.data ? (
					<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
						{t("overview.dataLoadFailed")}
					</div>
				) : (
					<CallAnalysisCard
						charts={callCharts.data}
						subtitle={windowSubtitle(windows.call)}
						granularity={callGranularity}
					/>
				)}
			</div>

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div className="flex flex-wrap items-center justify-between gap-2">
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.token)}</p>
					<RaceWindowControl
						state={windows.token}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, token: { ...prev.token, ...patch } }))
						}
					/>
				</div>
				{tokenCharts.isLoading ? (
					<Skeleton className="h-[260px] w-full" />
				) : tokenCharts.isError || !tokenCharts.data ? (
					<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
						{t("overview.dataLoadFailed")}
					</div>
				) : (
					<TokenAnalysisCard
						charts={tokenCharts.data}
						subtitle={windowSubtitle(windows.token)}
						granularity={tokenGranularity}
					/>
				)}
			</div>

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
					<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
						{t("overview.dataLoadFailed")}
					</div>
				) : (
					<InsightAnalysisCard
						data={insightQuery.data}
						subtitle={windowSubtitle(windows.insight)}
						granularity={insightGranularity}
					/>
				)}
			</div>

			{/* 成员模型赛马：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>{t("dashboard.memberRace")}</CardTitle>
						<p className="text-xs text-muted-foreground">{windowSubtitle(windows.race)}</p>
					</div>
					<RaceWindowControl
						state={windows.race}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, race: { ...prev.race, ...patch } }))
						}
					/>
				</CardHeader>
				<CardContent>
					<MemberModelRaceTable
						virtualModelId={virtualModelId}
						windowState={windows.race}
						now={now}
					/>
				</CardContent>
			</Card>
		</div>
	);
}
