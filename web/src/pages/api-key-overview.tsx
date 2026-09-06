import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { ProviderModelRaceCard } from "@/components/provider-model-race/ProviderModelRaceCard";
import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import { initialWindowFromUrl } from "@/components/race-window-control";
import {
	StatsSection,
	sectionGranularity,
	sectionWindow,
	useSectionSubtitle,
	useSectionWindows,
} from "@/components/stats-section";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelRaceCard } from "@/components/virtual-model-race/VirtualModelRaceCard";
import { useApiKeyDetail } from "@/hooks/use-api-keys";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useApiKeyMetrics } from "@/hooks/use-stats-metrics";
import { clientTzOffsetMinutes } from "@/lib/race-period";
import { KeyRound } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

const SECTION_KEYS = ["metrics", "call", "token", "insight"] as const;

/** API Key 数据面板：单 key 的请求指标聚合页（顶部 6 指标 + 调用/Token/性能可靠性）。 */
export default function ApiKeyOverviewPage() {
	const { t } = useTranslation();
	const navigate = useNavigate();
	const { id: idParam } = useParams();
	const apiKeyId = Number.parseInt(idParam ?? "", 10);
	const [searchParams] = useSearchParams();

	// 四块独立时间段，初始值来自 URL（无参数默认当天）；排行卡也用同一初始窗。
	const urlInitial = useMemo(() => initialWindowFromUrl(searchParams), [searchParams]);
	const { windows, now, setWindow } = useSectionWindows(SECTION_KEYS, () => urlInitial);
	const subtitle = useSectionSubtitle();

	// 先取 key detail 拿名称；已删除（404）→ 错误态引导返回列表。
	const idValid = Number.isFinite(apiKeyId);
	const detailQuery = useApiKeyDetail(idValid ? apiKeyId : null);
	const keyName = detailQuery.data?.name ?? null;

	const metricsWindow = sectionWindow(windows.metrics, now);
	const callWindow = sectionWindow(windows.call, now);
	const tokenWindow = sectionWindow(windows.token, now);
	const insightWindow = sectionWindow(windows.insight, now);

	// 图表桶粒度由所选时间窗口推导，并与本地时区偏移一起传给后端。
	const tzOffsetMinutes = clientTzOffsetMinutes();
	const callGranularity = sectionGranularity(windows.call, callWindow);
	const tokenGranularity = sectionGranularity(windows.token, tokenWindow);
	const insightGranularity = sectionGranularity(windows.insight, insightWindow);

	// key 解析前不发指标/图表请求（query 无 name 参数无意义）。
	const keyReady = keyName !== null;
	const apiKeyMetrics = useApiKeyMetrics(keyName, metricsWindow, keyReady);
	const callCharts = useDashboardCharts(
		{
			startTime: callWindow.startTime,
			endTime: callWindow.endTime,
			apiKey: keyName ?? undefined,
			granularity: callGranularity,
			tzOffsetMinutes,
		},
		keyReady,
	);
	const tokenCharts = useDashboardCharts(
		{
			startTime: tokenWindow.startTime,
			endTime: tokenWindow.endTime,
			apiKey: keyName ?? undefined,
			granularity: tokenGranularity,
			tzOffsetMinutes,
		},
		keyReady,
	);
	const insightQuery = useDashboardInsight(
		{
			startTime: insightWindow.startTime,
			endTime: insightWindow.endTime,
			apiKey: keyName ?? undefined,
			granularity: insightGranularity,
			tzOffsetMinutes,
		},
		keyReady,
	);

	// key 已删除 / id 非法：detail 失败即错误态（重试无意义，引导返回列表）。
	if (!idValid || detailQuery.isError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={KeyRound} title={t("apiKeys.overviewNotFoundTitle")} />
				<ErrorState description={t("apiKeys.overviewNotFoundDesc")} />
				<div className="flex justify-center">
					<Button variant="outline" size="sm" onClick={() => navigate("/api-keys")}>
						{t("apiKeys.backToList")}
					</Button>
				</div>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader
				icon={KeyRound}
				title={
					keyName !== null
						? `${keyName} · ${t("dashboardPage.titleSuffix")}`
						: t("apiKeys.overviewLoadingTitle")
				}
			/>

			{/* 顶部：6 指标概览（独立时间段；key 名解析后加载） */}
			{keyName === null ? (
				<Skeleton className="h-[240px] w-full" />
			) : (
				<MetricsSummaryCard
					data={apiKeyMetrics.data}
					isLoading={apiKeyMetrics.isLoading}
					windowState={windows.metrics}
					now={now}
					onWindowChange={setWindow("metrics")}
					subtitle={subtitle(windows.metrics, now)}
				/>
			)}

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳） */}
			{keyName !== null && (
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
			)}

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳） */}
			{keyName !== null && (
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
			)}

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳） */}
			{keyName !== null && (
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

			{/* 排行：该 key 用到的虚拟模型 / 供应商 / 模型（各自独立时间窗 + 行深链） */}
			{keyName !== null && (
				<>
					<VirtualModelRaceCard apiKey={keyName} initialWindow={urlInitial} />
					<ProviderRaceCard apiKey={keyName} initialWindow={urlInitial} />
					<ProviderModelRaceCard apiKey={keyName} initialWindow={urlInitial} />
				</>
			)}
		</div>
	);
}
