import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { initialWindowFromUrl } from "@/components/race-window-control";
import { type RaceWindowState, windowQueryString } from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import {
	AnalysisSections,
	CardStatsSection,
	queryWindowGranularity,
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
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

const SECTION_KEYS = ["metrics", "call", "token", "race", "insight"] as const;

/** 成员模型赛马表格（配置成员全量 + 6 指标 + 排序；停用成员灰显）。 */
function MemberModelRaceTable({
	virtualModelId,
	windowState,
	tz,
}: {
	virtualModelId: number;
	windowState: RaceWindowState;
	tz: string;
}) {
	const navigate = useNavigate();
	const { t } = useTranslation();
	const { sort, onSort } = useRaceSort();
	const window = sectionWindow(windowState, tz);
	const query = useVirtualModelMemberRank(window, sort, true, virtualModelId);

	const openModelOverview = (item: VirtualModelMemberRankItem) => {
		if (item.modelPk === null || item.modelPk === undefined) {
			return;
		}
		const custom = windowState.appliedCustom ?? {
			startTime: windowState.customStart,
			endTime: windowState.customEnd,
		};
		navigate(`/models/${item.modelPk}/overview?${windowQueryString(windowState, custom)}`);
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
	const [searchParams] = useSearchParams();
	// 16-09：排行卡与区块共享同一 URL 初始窗（此前排行卡固定当天，深链时不一致）。
	const urlInitial = useMemo(() => initialWindowFromUrl(searchParams), [searchParams]);
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

	const metricsWindow = sectionWindow(windows.metrics, tz);
	const callWindow = sectionWindow(windows.call, tz);
	const tokenWindow = sectionWindow(windows.token, tz);
	const insightWindow = sectionWindow(windows.insight, tz);

	const vmMetrics = useVirtualModelMetrics(virtualModelId, metricsWindow, keyReady);

	// 图表桶粒度由所选时间窗口推导（分桶时区由后端按设置表解释）。
	const callGranularity = queryWindowGranularity(windows.call, callWindow);
	const tokenGranularity = queryWindowGranularity(windows.token, tokenWindow);
	const insightGranularity = queryWindowGranularity(windows.insight, insightWindow);

	const callCharts = useDashboardCharts(
		{
			window: callWindow,
			virtualModelId,
			granularity: callGranularity,
		},
		keyReady,
	);
	const tokenCharts = useDashboardCharts(
		{
			window: tokenWindow,
			virtualModelId,
			granularity: tokenGranularity,
		},
		keyReady,
	);
	const insightQuery = useDashboardInsight(
		{
			window: insightWindow,
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

			{/* 调用 / Token / 性能与可靠性分析：各自独立时间段 */}
			<AnalysisSections
				now={now}
				windows={windows}
				onWindowChange={setWindow}
				granularities={{
					call: callGranularity,
					token: tokenGranularity,
					insight: insightGranularity,
				}}
				callCharts={callCharts}
				tokenCharts={tokenCharts}
				insight={insightQuery}
			/>

			{/* API Key 赛马：独立时间段（可靠性分析之下、成员模型赛马之上） */}
			<ApiKeyRaceCard
				filter={Number.isFinite(virtualModelId) ? { virtualModelId } : undefined}
				initialWindow={urlInitial}
			/>

			{/* 成员模型赛马：独立时间段 */}
			<CardStatsSection
				title={t("dashboard.memberRace")}
				now={now}
				windowState={windows.race}
				onWindowChange={setWindow("race")}
			>
				<MemberModelRaceTable virtualModelId={virtualModelId} windowState={windows.race} tz={tz} />
			</CardStatsSection>
		</div>
	);
}
