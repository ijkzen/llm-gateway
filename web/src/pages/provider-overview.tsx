import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { ProviderUsageCard, usageEnabled } from "@/components/providers/ProviderUsageCard";
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
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { type ProviderModelRankItem, useProviderModelRace } from "@/hooks/use-provider-model-race";
import { useProviderDetail } from "@/hooks/use-providers";
import { useProviderMetrics } from "@/hooks/use-stats-metrics";
import { useStatsTimeZone } from "@/hooks/use-stats-time-zone";
import { useUsageEstimate } from "@/hooks/use-usage-estimate";
import { Boxes } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

const SECTION_KEYS = ["metrics", "call", "token", "race", "insight"] as const;

/** 供应商内部模型赛马表格（按供应商过滤 + 6 指标 + 排序）。 */
function InternalModelRaceTable({
	providerId,
	windowState,
	tz,
}: {
	providerId: number;
	windowState: RaceWindowState;
	tz: string;
}) {
	const navigate = useNavigate();
	const { sort, onSort } = useRaceSort();
	const window = sectionWindow(windowState, tz);
	const query = useProviderModelRace(window, sort, true, providerId);

	const openModelOverview = (item: ProviderModelRankItem) => {
		if (item.modelPk === null || item.modelPk === undefined) {
			return;
		}
		const custom = windowState.appliedCustom ?? {
			startTime: windowState.customStart,
			endTime: windowState.customEnd,
		};
		navigate(`/models/${item.modelPk}/overview?${windowQueryString(windowState, custom)}`);
	};

	return (
		<SortableMetricTable
			items={query.data?.items ?? []}
			sort={sort}
			onSort={onSort}
			nameHeader="dashboard.modelColumn"
			renderName={(item) => item.modelId}
			rowKey={(item) => item.modelId}
			onRowClick={openModelOverview}
			isRowClickable={(item) => item.modelPk !== null && item.modelPk !== undefined}
			rowTitleKey="race.openModelDetail"
		/>
	);
}

/** 供应商二级数据面板：调用分析 + token 分析 + 内部模型赛马，三块独立时间段。 */
export default function ProviderOverviewPage() {
	const { t } = useTranslation();
	const { providerId: providerIdParam } = useParams();
	const [searchParams] = useSearchParams();
	// 16-09：排行卡与区块共享同一 URL 初始窗（此前排行卡固定当天，深链时不一致）。
	const urlInitial = useMemo(() => initialWindowFromUrl(searchParams), [searchParams]);
	const navigate = useNavigate();
	const providerId = Number.parseInt(providerIdParam ?? "", 10);
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS);
	const subtitle = useSectionSubtitle();
	const tz = useStatsTimeZone();

	// 16-02：与 api-key/model 两页对齐——id 非法或 detail 失败（已删除）走错误态。
	const idValid = Number.isFinite(providerId);
	const providerDetail = useProviderDetail(idValid ? providerId : null);
	const providerName =
		providerDetail.data?.name ?? t("dashboardPage.providerLabel", { id: providerId });
	const keyReady = idValid && !providerDetail.isError;

	const metricsWindow = sectionWindow(windows.metrics, tz);
	const callWindow = sectionWindow(windows.call, tz);
	const tokenWindow = sectionWindow(windows.token, tz);
	const insightWindow = sectionWindow(windows.insight, tz);

	// 图表桶粒度由所选时间窗口推导（分桶时区由后端按设置表解释）。
	const callGranularity = queryWindowGranularity(windows.call, callWindow);
	const tokenGranularity = queryWindowGranularity(windows.token, tokenWindow);
	const insightGranularity = queryWindowGranularity(windows.insight, insightWindow);

	const providerMetrics = useProviderMetrics(providerId, metricsWindow, keyReady);
	// 订阅制 + 开启用量时才有预估；非订阅制后端返回 400，此处直接禁用。
	const showUsage =
		providerDetail.data?.billingMode === 1 && usageEnabled(providerDetail.data.extra);
	const usageEstimate = useUsageEstimate(showUsage ? providerId : null);

	const callCharts = useDashboardCharts(
		{
			window: callWindow,
			providerId,
			granularity: callGranularity,
		},
		keyReady,
	);
	const tokenCharts = useDashboardCharts(
		{
			window: tokenWindow,
			providerId,
			granularity: tokenGranularity,
		},
		keyReady,
	);
	const insightQuery = useDashboardInsight(
		{
			window: insightWindow,
			providerId,
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
					<Button variant="outline" size="sm" onClick={() => navigate("/providers")}>
						{t("dashboardPage.backToList")}
					</Button>
				</div>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader icon={Boxes} title={`${providerName} · ${t("dashboardPage.titleSuffix")}`} />

			{/* 顶部：6 指标概览（独立时间段）+ 订阅制用量卡（含 Token 预估） */}
			<MetricsSummaryCard
				data={providerMetrics.data}
				isLoading={providerMetrics.isLoading}
				windowState={windows.metrics}
				now={now}
				onWindowChange={setWindow("metrics")}
				subtitle={subtitle(windows.metrics, now)}
				extra={
					showUsage && (
						<div className="mt-4">
							<ProviderUsageCard providerId={providerId} estimate={usageEstimate.data} />
						</div>
					)
				}
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

			{/* API Key 赛马：独立时间段（可靠性分析之下、内部模型赛马之上） */}
			<ApiKeyRaceCard
				filter={Number.isFinite(providerId) ? { providerId } : undefined}
				initialWindow={urlInitial}
			/>

			{/* 供应商内部模型赛马：独立时间段 */}
			<CardStatsSection
				title={t("dashboard.internalModelRace")}
				now={now}
				windowState={windows.race}
				onWindowChange={setWindow("race")}
			>
				<InternalModelRaceTable providerId={providerId} windowState={windows.race} tz={tz} />
			</CardStatsSection>
		</div>
	);
}
