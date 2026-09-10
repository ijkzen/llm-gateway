import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import {
	type RaceWindowState,
	raceWindowBounds,
	windowQueryString,
} from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import {
	CardStatsSection,
	StatsSection,
	sectionGranularity,
	sectionWindow,
	useSectionSubtitle,
	useSectionWindows,
} from "@/components/stats-section";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useVirtualModelMetrics } from "@/hooks/use-stats-metrics";
import { useStatsTimeZone } from "@/hooks/use-stats-time-zone";
import {
	type VirtualModelMemberRankItem,
	useVirtualModelMemberRank,
} from "@/hooks/use-virtual-model-member-rank";
import { useVirtualModelDetail } from "@/hooks/use-virtual-models";
import { Boxes } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router-dom";

const SECTION_KEYS = ["metrics", "call", "token", "race", "insight"] as const;

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
	const tz = useStatsTimeZone();
	const window = raceWindowBounds(windowState, now, tz);
	const query = useVirtualModelMemberRank(window, sort, true, virtualModelId);

	const openModelOverview = (item: VirtualModelMemberRankItem) => {
		if (item.modelPk === null || item.modelPk === undefined) {
			return;
		}
		navigate(`/models/${item.modelPk}/overview?${windowQueryString(windowState, window)}`);
	};

	if (query.isLoading) {
		return <SkeletonFallback />;
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

function SkeletonFallback() {
	return <Skeleton className="h-[220px] w-full" />;
}

/** 虚拟模型二级数据面板：调用分析 + token 分析 + 成员模型赛马，三块独立时间段。 */
export default function VirtualModelOverviewPage() {
	const { t } = useTranslation();
	const { virtualModelId: virtualModelIdParam } = useParams();
	const navigate = useNavigate();
	const virtualModelId = Number.parseInt(virtualModelIdParam ?? "", 10);
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS);
	const subtitle = useSectionSubtitle();
	const tz = useStatsTimeZone();

	// 16-02：与 api-key/model 两页对齐——id 非法或 detail 失败（已删除）走错误态。
	const idValid = Number.isFinite(virtualModelId);
	const detail = useVirtualModelDetail(idValid ? virtualModelId : null);
	const displayId =
		detail.data?.displayId ?? t("dashboardPage.virtualModelLabel", { id: virtualModelId });
	const keyReady = idValid && !detail.isError;

	const metricsWindow = sectionWindow(windows.metrics, now, tz);
	const callWindow = sectionWindow(windows.call, now, tz);
	const tokenWindow = sectionWindow(windows.token, now, tz);
	const insightWindow = sectionWindow(windows.insight, now, tz);

	const vmMetrics = useVirtualModelMetrics(virtualModelId, metricsWindow, keyReady);

	// 图表桶粒度由所选时间窗口推导（分桶时区由后端按设置表解释）。
	const callGranularity = sectionGranularity(windows.call, callWindow);
	const tokenGranularity = sectionGranularity(windows.token, tokenWindow);
	const insightGranularity = sectionGranularity(windows.insight, insightWindow);

	const callCharts = useDashboardCharts(
		{
			startTime: callWindow.startTime,
			endTime: callWindow.endTime,
			virtualModelId,
			granularity: callGranularity,
		},
		keyReady,
	);
	const tokenCharts = useDashboardCharts(
		{
			startTime: tokenWindow.startTime,
			endTime: tokenWindow.endTime,
			virtualModelId,
			granularity: tokenGranularity,
		},
		keyReady,
	);
	const insightQuery = useDashboardInsight(
		{
			startTime: insightWindow.startTime,
			endTime: insightWindow.endTime,
			virtualModelId,
			granularity: insightGranularity,
		},
		keyReady,
	);

	if (!keyReady) {
		return (
			<div className="space-y-6">
				<PageHeader icon={Boxes} title={t("dashboardPage.overviewNotFoundTitle")} />
				<ErrorState description={t("dashboardPage.overviewNotFoundDesc")} />
				<div className="flex justify-center">
					<Button variant="outline" size="sm" onClick={() => navigate("/virtual-models")}>
						{t("dashboardPage.backToList")}
					</Button>
				</div>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader icon={Boxes} title={`${displayId} · ${t("dashboardPage.titleSuffix")}`} />

			{/* 顶部：6 指标概览（独立时间段；虚拟模型无用量信息） */}
			<MetricsSummaryCard
				data={vmMetrics.data}
				isLoading={vmMetrics.isLoading}
				windowState={windows.metrics}
				now={now}
				onWindowChange={setWindow("metrics")}
				subtitle={subtitle(windows.metrics, now)}
			/>

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳） */}
			<StatsSection
				now={now}
				windowState={windows.call}
				onWindowChange={setWindow("call")}
				status={{
					isLoading: callCharts.isLoading,
					isError: callCharts.isError || !callCharts.data,
					onRetry: () => callCharts.refetch(),
				}}
			>
				{callCharts.data && (
					<CallAnalysisCard
						charts={callCharts.data}
						subtitle={subtitle(windows.call, now)}
						granularity={callGranularity}
					/>
				)}
			</StatsSection>

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳） */}
			<StatsSection
				now={now}
				windowState={windows.token}
				onWindowChange={setWindow("token")}
				status={{
					isLoading: tokenCharts.isLoading,
					isError: tokenCharts.isError || !tokenCharts.data,
					onRetry: () => tokenCharts.refetch(),
				}}
			>
				{tokenCharts.data && (
					<TokenAnalysisCard
						charts={tokenCharts.data}
						subtitle={subtitle(windows.token, now)}
						granularity={tokenGranularity}
					/>
				)}
			</StatsSection>

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳） */}
			<StatsSection
				now={now}
				windowState={windows.insight}
				onWindowChange={setWindow("insight")}
				status={{
					isLoading: insightQuery.isLoading,
					isError: insightQuery.isError || !insightQuery.data,
					onRetry: () => insightQuery.refetch(),
				}}
			>
				{insightQuery.data && (
					<InsightAnalysisCard
						data={insightQuery.data}
						subtitle={subtitle(windows.insight, now)}
						granularity={insightGranularity}
					/>
				)}
			</StatsSection>

			{/* API Key 赛马：独立时间段（可靠性分析之下、成员模型赛马之上） */}
			<ApiKeyRaceCard filter={Number.isFinite(virtualModelId) ? { virtualModelId } : undefined} />

			{/* 成员模型赛马：独立时间段 */}
			<CardStatsSection
				title={t("dashboard.memberRace")}
				now={now}
				windowState={windows.race}
				onWindowChange={setWindow("race")}
			>
				<MemberModelRaceTable
					virtualModelId={virtualModelId}
					windowState={windows.race}
					now={now}
				/>
			</CardStatsSection>
		</div>
	);
}
