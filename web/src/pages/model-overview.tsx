import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import { TrendLineChart } from "@/components/dashboard-charts";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { initialWindowFromUrl } from "@/components/race-window-control";
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
import { useModelMetrics } from "@/hooks/use-model-metrics";
import { useProviderModelDetail } from "@/hooks/use-provider-models";
import { useStatsTimeZone } from "@/hooks/use-stats-time-zone";
import { formatTokenCount, localeOf } from "@/lib/utils";
import { TrendingUp } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

const SECTION_KEYS = ["call", "token", "metrics", "insight"] as const;

/**
 * 模型详情三级页：路由携带 provider_model 自增主键，先经 detail 取回所属供应商
 * 与远端字符串模型 ID，再驱动各指标/图表请求（stats 侧仍按 provider + 字符串
 * model_id 过滤，见 request 表关联键）。detail 失败（模型已删/非法 id）→ 错误态。
 */
export default function ModelOverviewPage() {
	const { t, i18n } = useTranslation();
	const navigate = useNavigate();
	const { modelId: modelIdParam } = useParams();
	const [searchParams] = useSearchParams();
	// 16-09：排行卡与区块共享同一 URL 初始窗（此前排行卡固定当天，深链时不一致）。
	const urlInitial = useMemo(() => initialWindowFromUrl(searchParams), [searchParams]);
	const modelId = Number.parseInt(modelIdParam ?? "", 10);
	const idValid = Number.isFinite(modelId);
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS);
	const subtitle = useSectionSubtitle();

	// detail 未就绪前不发任何 stats 请求（图表需要字符串 modelId 与 providerId）。
	const detailQuery = useProviderModelDetail(idValid ? modelId : null);
	const modelDetail = detailQuery.data;

	const tz = useStatsTimeZone();
	const callWindow = sectionWindow(windows.call, now, tz);
	const tokenWindow = sectionWindow(windows.token, now, tz);
	const metricsWindow = sectionWindow(windows.metrics, now, tz);
	const insightWindow = sectionWindow(windows.insight, now, tz);

	// 图表桶粒度由所选时间窗口推导（分桶时区由后端按设置表解释）。
	const callGranularity = sectionGranularity(windows.call, callWindow);
	const tokenGranularity = sectionGranularity(windows.token, tokenWindow);
	const insightGranularity = sectionGranularity(windows.insight, insightWindow);

	const detailReady = modelDetail !== undefined;
	const providerId = modelDetail?.providerId ?? -1;
	const remoteModelId = modelDetail?.providerModelId ?? "";

	const callCharts = useDashboardCharts(
		{
			startTime: callWindow.startTime,
			endTime: callWindow.endTime,
			providerId,
			modelId: remoteModelId,
			granularity: callGranularity,
		},
		detailReady,
	);
	const tokenCharts = useDashboardCharts(
		{
			startTime: tokenWindow.startTime,
			endTime: tokenWindow.endTime,
			providerId,
			modelId: remoteModelId,
			granularity: tokenGranularity,
		},
		detailReady,
	);
	const insightQuery = useDashboardInsight(
		{
			startTime: insightWindow.startTime,
			endTime: insightWindow.endTime,
			providerId,
			modelId: remoteModelId,
			granularity: insightGranularity,
		},
		detailReady,
	);
	const metrics = useModelMetrics(providerId, remoteModelId, metricsWindow, detailReady);

	// 模型已删除 / id 非法：detail 失败即错误态（重试无意义，引导返回列表）。
	if (!idValid || detailQuery.isError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={TrendingUp} title={t("providerModels.overviewNotFoundTitle")} />
				<ErrorState description={t("providerModels.overviewNotFoundDesc")} />
				<div className="flex justify-center">
					<Button variant="outline" size="sm" onClick={() => navigate("/provider-models")}>
						{t("providerModels.backToModels")}
					</Button>
				</div>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				icon={TrendingUp}
				title={
					modelDetail
						? `${modelDetail.providerName || t("dashboardPage.providerFallback")}・${modelDetail.providerModelId} · ${t("dashboardPage.modelTitleSuffix")}`
						: t("providerModels.overviewLoadingTitle")
				}
			/>

			{/* 单模型指标卡片：独立时间段（置顶，概览优先）；detail 解析后加载 */}
			{modelDetail === undefined ? (
				<Skeleton className="h-[240px] w-full" />
			) : (
				<MetricsSummaryCard
					data={metrics.data}
					isLoading={metrics.isLoading}
					windowState={windows.metrics}
					now={now}
					onWindowChange={setWindow("metrics")}
					subtitle={subtitle(windows.metrics, now)}
					title={t("dashboard.modelMetric")}
				/>
			)}

			{/* 调用分析折线（仅折线）：独立时间段 */}
			{modelDetail !== undefined && (
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
			)}

			{/* Token 折线（仅折线）：独立时间段 */}
			{modelDetail !== undefined && (
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
						formatValue={(v) => formatTokenCount(v, localeOf(i18n.language))}
						kind="tokens"
						granularity={tokenGranularity}
					/>
				</CardStatsSection>
			)}

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳） */}
			{modelDetail !== undefined && (
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
			)}

			{/* API Key 赛马：独立时间段（按当前供应商+模型过滤） */}
			{modelDetail !== undefined && (
				<ApiKeyRaceCard
					filter={{ providerId, modelId: remoteModelId }}
					initialWindow={urlInitial}
				/>
			)}
		</div>
	);
}
