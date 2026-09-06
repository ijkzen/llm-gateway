import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import {
	RaceWindowControl,
	type RaceWindowState,
	initialWindowFromUrl,
	raceWindowBounds,
	windowQueryString,
} from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useVirtualModelMetrics } from "@/hooks/use-stats-metrics";
import {
	type VirtualModelMemberRankItem,
	useVirtualModelMemberRank,
} from "@/hooks/use-virtual-model-member-rank";
import { useVirtualModelDetail } from "@/hooks/use-virtual-models";
import { chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { Boxes } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

/** 二级页六个图表区块的独立时间段状态。 */
interface VirtualModelOverviewWindows {
	metrics: RaceWindowState;
	call: RaceWindowState;
	token: RaceWindowState;
	race: RaceWindowState;
	insight: RaceWindowState;
}

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
	const { sort, onSort } = useRaceSort();
	const window = raceWindowBounds(windowState, now);
	const query = useVirtualModelMemberRank(window, sort, true, virtualModelId);

	const openModelOverview = (item: VirtualModelMemberRankItem) => {
		navigate(
			`/models/${item.providerId}/${encodeURIComponent(item.modelId)}/overview?${windowQueryString(windowState, window)}`,
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
		<SortableMetricTable
			items={query.data.items}
			sort={sort}
			onSort={onSort}
			nameHeader="dashboard.providerModel"
			renderName={(item) => (
				<>
					{item.providerName || t("race.unknownProvider")}・{item.modelId}
					{!item.memberEnable && (
						<span className="ml-2 text-xs text-muted-foreground">{t("race.disabledSuffix")}</span>
					)}
				</>
			)}
			rowKey={(item) => `${item.providerName}::${item.modelId}`}
			onRowClick={openModelOverview}
			rowTitleKey="race.openModelDetail"
			rowClassName={(item) => (item.memberEnable ? "" : "opacity-50")}
		/>
	);
}

/** 虚拟模型二级数据面板：调用分析 + token 分析 + 成员模型赛马，三块独立时间段。 */
export default function VirtualModelOverviewPage() {
	const { t } = useTranslation();
	const { virtualModelId: virtualModelIdParam } = useParams();
	const virtualModelId = Number.parseInt(virtualModelIdParam ?? "", 10);
	const [searchParams] = useSearchParams();

	// 六块独立时间段，初始值来自 URL（无参数默认当天）。
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
			<PageHeader icon={Boxes} title={`${displayId} · ${t("dashboardPage.titleSuffix")}`} />

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
					<ErrorState onRetry={() => callCharts.refetch()} />
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
					<ErrorState onRetry={() => tokenCharts.refetch()} />
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
					<ErrorState onRetry={() => insightQuery.refetch()} />
				) : (
					<InsightAnalysisCard
						data={insightQuery.data}
						subtitle={windowSubtitle(windows.insight)}
						granularity={insightGranularity}
					/>
				)}
			</div>

			{/* API Key 赛马：独立时间段（可靠性分析之下、成员模型赛马之上） */}
			<ApiKeyRaceCard filter={Number.isFinite(virtualModelId) ? { virtualModelId } : undefined} />

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
