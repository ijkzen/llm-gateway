import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { ProviderUsageCard, usageEnabled } from "@/components/providers/ProviderUsageCard";
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
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { type ProviderModelRankItem, useProviderModelRace } from "@/hooks/use-provider-model-race";
import { useProviderDetail } from "@/hooks/use-providers";
import { useProviderMetrics } from "@/hooks/use-stats-metrics";
import { useUsageEstimate } from "@/hooks/use-usage-estimate";
import { clientTzOffsetMinutes } from "@/lib/race-period";
import { Boxes } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router-dom";

const SECTION_KEYS = ["metrics", "call", "token", "race", "insight"] as const;

/** 供应商内部模型赛马表格（按供应商过滤 + 6 指标 + 排序）。 */
function InternalModelRaceTable({
	providerId,
	windowState,
	now,
}: {
	providerId: number;
	windowState: RaceWindowState;
	now: number;
}) {
	const navigate = useNavigate();
	const { sort, onSort } = useRaceSort();
	const window = raceWindowBounds(windowState, now);
	const query = useProviderModelRace(window, sort, true, providerId);

	const openModelOverview = (item: ProviderModelRankItem) => {
		if (item.modelPk === null || item.modelPk === undefined) {
			return;
		}
		navigate(`/models/${item.modelPk}/overview?${windowQueryString(windowState, window)}`);
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
	const providerId = Number.parseInt(providerIdParam ?? "", 10);
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS);
	const subtitle = useSectionSubtitle();

	const providerDetail = useProviderDetail(Number.isFinite(providerId) ? providerId : null);
	const providerName =
		providerDetail.data?.name ?? t("dashboardPage.providerLabel", { id: providerId });

	const metricsWindow = sectionWindow(windows.metrics, now);
	const callWindow = sectionWindow(windows.call, now);
	const tokenWindow = sectionWindow(windows.token, now);
	const insightWindow = sectionWindow(windows.insight, now);

	// 图表桶粒度由所选时间窗口推导，并与本地时区偏移一起传给后端。
	const tzOffsetMinutes = clientTzOffsetMinutes();
	const callGranularity = sectionGranularity(windows.call, callWindow);
	const tokenGranularity = sectionGranularity(windows.token, tokenWindow);
	const insightGranularity = sectionGranularity(windows.insight, insightWindow);

	const providerMetrics = useProviderMetrics(
		providerId,
		metricsWindow,
		Number.isFinite(providerId),
	);
	// 订阅制 + 开启用量时才有预估；非订阅制后端返回 400，此处直接禁用。
	const showUsage =
		providerDetail.data?.billingMode === 1 && usageEnabled(providerDetail.data.extra);
	const usageEstimate = useUsageEstimate(showUsage ? providerId : null);

	const callCharts = useDashboardCharts({
		startTime: callWindow.startTime,
		endTime: callWindow.endTime,
		providerId,
		granularity: callGranularity,
		tzOffsetMinutes,
	});
	const tokenCharts = useDashboardCharts({
		startTime: tokenWindow.startTime,
		endTime: tokenWindow.endTime,
		providerId,
		granularity: tokenGranularity,
		tzOffsetMinutes,
	});
	const insightQuery = useDashboardInsight({
		startTime: insightWindow.startTime,
		endTime: insightWindow.endTime,
		providerId,
		granularity: insightGranularity,
		tzOffsetMinutes,
	});

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

			{/* API Key 赛马：独立时间段（可靠性分析之下、内部模型赛马之上） */}
			<ApiKeyRaceCard filter={Number.isFinite(providerId) ? { providerId } : undefined} />

			{/* 供应商内部模型赛马：独立时间段 */}
			<CardStatsSection
				title={t("dashboard.internalModelRace")}
				now={now}
				windowState={windows.race}
				onWindowChange={setWindow("race")}
			>
				<InternalModelRaceTable providerId={providerId} windowState={windows.race} now={now} />
			</CardStatsSection>
		</div>
	);
}
