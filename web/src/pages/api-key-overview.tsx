import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { MetricsSummaryCard } from "@/components/dashboard/metrics-summary-card";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import { PageHeader } from "@/components/page-header";
import { ProviderModelRaceCard } from "@/components/provider-model-race/ProviderModelRaceCard";
import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import {
	RaceWindowControl,
	type RaceWindowState,
	initialWindowFromUrl,
	raceWindowBounds,
} from "@/components/race-window-control";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelRaceCard } from "@/components/virtual-model-race/VirtualModelRaceCard";
import { useApiKeyDetail } from "@/hooks/use-api-keys";
import { useDashboardInsight } from "@/hooks/use-dashboard-insight";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import { useApiKeyMetrics } from "@/hooks/use-stats-metrics";
import { chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { KeyRound } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

/** API Key 数据面板区块的独立时间段状态。 */
interface ApiKeyOverviewWindows {
	metrics: RaceWindowState;
	call: RaceWindowState;
	token: RaceWindowState;
	insight: RaceWindowState;
}

/** API Key 数据面板：单 key 的请求指标聚合页（顶部 6 指标 + 调用/Token/性能可靠性）。 */
export default function ApiKeyOverviewPage() {
	const { t } = useTranslation();
	const navigate = useNavigate();
	const { id: idParam } = useParams();
	const apiKeyId = Number.parseInt(idParam ?? "", 10);
	const [searchParams] = useSearchParams();

	// 四块独立时间段，初始值来自 URL（无参数默认当天）；排行卡也用同一初始窗。
	const urlInitial = initialWindowFromUrl(searchParams);
	const [windows, setWindows] = useState<ApiKeyOverviewWindows>(() => {
		return {
			metrics: { ...urlInitial },
			call: { ...urlInitial },
			token: { ...urlInitial },
			insight: { ...urlInitial },
		};
	});
	// 各块固化 now（标题稳定）。
	const [now] = useState(() => Date.now());

	// 先取 key detail 拿名称；已删除（404）→ 错误态引导返回列表。
	const idValid = Number.isFinite(apiKeyId);
	const detailQuery = useApiKeyDetail(idValid ? apiKeyId : null);
	const keyName = detailQuery.data?.name ?? null;

	const metricsWindow = raceWindowBounds(windows.metrics, now);
	const callWindow = raceWindowBounds(windows.call, now);
	const tokenWindow = raceWindowBounds(windows.token, now);
	const insightWindow = raceWindowBounds(windows.insight, now);

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

	const windowSubtitle = (state: RaceWindowState) =>
		state.period === "custom"
			? t("overview.customWindow")
			: formatPeriodLabel(state.period, state.offset, now);

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
					onWindowChange={(patch) =>
						setWindows((prev) => ({ ...prev, metrics: { ...prev.metrics, ...patch } }))
					}
					subtitle={windowSubtitle(windows.metrics)}
				/>
			)}

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳） */}
			{keyName !== null && (
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
			)}

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳） */}
			{keyName !== null && (
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
			)}

			{/* 性能与可靠性分析：独立时间段（InsightAnalysisCard 自带卡片壳） */}
			{keyName !== null && (
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
