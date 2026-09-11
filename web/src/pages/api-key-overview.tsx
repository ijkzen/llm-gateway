import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { ProviderModelRaceCard } from "@/components/provider-model-race/ProviderModelRaceCard";
import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import { initialWindowFromUrl } from "@/components/race-window-control";
import {
	AnalysisSections,
	queryWindowGranularity,
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
import { useStatsTimeZone } from "@/hooks/use-stats-time-zone";
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

	const tz = useStatsTimeZone();
	const metricsWindow = sectionWindow(windows.metrics, tz);
	const callWindow = sectionWindow(windows.call, tz);
	const tokenWindow = sectionWindow(windows.token, tz);
	const insightWindow = sectionWindow(windows.insight, tz);

	// 图表桶粒度由所选时间窗口推导（分桶时区由后端按设置表解释）。
	const callGranularity = queryWindowGranularity(windows.call, callWindow);
	const tokenGranularity = queryWindowGranularity(windows.token, tokenWindow);
	const insightGranularity = queryWindowGranularity(windows.insight, insightWindow);

	// key 解析前不发指标/图表请求（query 无 name 参数无意义）。
	const keyReady = keyName !== null;
	const apiKeyMetrics = useApiKeyMetrics(keyName, metricsWindow, keyReady);
	const callCharts = useDashboardCharts(
		{
			window: callWindow,
			apiKey: keyName ?? undefined,
			granularity: callGranularity,
		},
		keyReady,
	);
	const tokenCharts = useDashboardCharts(
		{
			window: tokenWindow,
			apiKey: keyName ?? undefined,
			granularity: tokenGranularity,
		},
		keyReady,
	);
	const insightQuery = useDashboardInsight(
		{
			window: insightWindow,
			apiKey: keyName ?? undefined,
			granularity: insightGranularity,
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

			{/* 调用 / Token / 性能与可靠性分析：各自独立时间段 */}
			{keyName !== null && (
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
