import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { TrendLineChart } from "@/components/dashboard-charts";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
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
import { useModelMetrics } from "@/hooks/use-model-metrics";
import { clientTzOffsetMinutes } from "@/lib/race-period";
import { formatTokenCount } from "@/lib/utils";
import { TrendingUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router-dom";

const SECTION_KEYS = ["call", "token", "metrics", "insight"] as const;

/** 模型详情三级页：单模型指标卡片（置顶）+ 调用分析折线 + Token 折线，三块独立时间段。 */
export default function ModelOverviewPage() {
	const { t } = useTranslation();
	const { providerId: providerIdParam, modelId: modelIdParam } = useParams();
	const providerId = Number.parseInt(providerIdParam ?? "", 10);
	const modelId = decodeURIComponent(modelIdParam ?? "");
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS);
	const subtitle = useSectionSubtitle();

	const callWindow = sectionWindow(windows.call, now);
	const tokenWindow = sectionWindow(windows.token, now);
	const metricsWindow = sectionWindow(windows.metrics, now);
	const insightWindow = sectionWindow(windows.insight, now);

	// 图表桶粒度由所选时间窗口推导，并与本地时区偏移一起传给后端。
	const tzOffsetMinutes = clientTzOffsetMinutes();
	const callGranularity = sectionGranularity(windows.call, callWindow);
	const tokenGranularity = sectionGranularity(windows.token, tokenWindow);
	const insightGranularity = sectionGranularity(windows.insight, insightWindow);

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

	const title = `${metrics.data?.providerName || t("dashboardPage.providerFallback")}・${modelId} · ${t("dashboardPage.modelTitleSuffix")}`;

	return (
		<div className="space-y-6">
			<PageHeader icon={TrendingUp} title={title} />

			{/* 单模型指标卡片：独立时间段（置顶，概览优先） */}
			<MetricsSummaryCard
				data={metrics.data}
				isLoading={metrics.isLoading}
				windowState={windows.metrics}
				now={now}
				onWindowChange={setWindow("metrics")}
				subtitle={subtitle(windows.metrics, now)}
				title={t("dashboard.modelMetric")}
			/>

			{/* 调用分析折线（仅折线）：独立时间段 */}
			<CardStatsSection
				title={t("dashboard.analysis")}
				now={now}
				windowState={windows.call}
				onWindowChange={setWindow("call")}
				status={{
					isLoading: callCharts.isLoading,
					isError: callCharts.isError || !callCharts.data,
					onRetry: () => callCharts.refetch(),
				}}
			>
				<TrendLineChart
					data={callCharts.data?.callTrend ?? []}
					label={t("overview.calls")}
					granularity={callGranularity}
				/>
			</CardStatsSection>

			{/* Token 折线（仅折线）：独立时间段 */}
			<CardStatsSection
				title={t("dashboard.tokenAnalysis")}
				now={now}
				windowState={windows.token}
				onWindowChange={setWindow("token")}
				status={{
					isLoading: tokenCharts.isLoading,
					isError: tokenCharts.isError || !tokenCharts.data,
					onRetry: () => tokenCharts.refetch(),
				}}
			>
				<TrendLineChart
					data={tokenCharts.data?.tokenTrend ?? []}
					label={t("overview.tokens")}
					formatValue={formatTokenCount}
					kind="tokens"
					granularity={tokenGranularity}
				/>
			</CardStatsSection>

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

			{/* API Key 赛马：独立时间段（按当前供应商+模型过滤） */}
			<ApiKeyRaceCard
				filter={
					Number.isFinite(providerId) && modelId.length > 0 ? { providerId, modelId } : undefined
				}
			/>
		</div>
	);
}
