import { EmptyState } from "@/components/empty-state";
import {
	FailureReasonBarChart,
	FailureTrendChart,
	OutputPerSecLineChart,
	PercentileLineChart,
	ThroughputChart,
	TokenStructureChart,
} from "@/components/insight-charts";
import { SegmentedControl } from "@/components/segmented-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { InsightData } from "@/hooks/use-dashboard-insight";
import type { ChartGranularity } from "@/lib/race-period";
import { formatTokenCount } from "@/lib/utils";
import { useState } from "react";
import { useTranslation } from "react-i18next";

type InsightTab = "failure" | "latency" | "token" | "throughput";

const INSIGHT_TABS = [
	{ value: "failure", labelKey: "dashboard.insightFailure" },
	{ value: "latency", labelKey: "dashboard.insightLatency" },
	{ value: "token", labelKey: "dashboard.insightToken" },
	{ value: "throughput", labelKey: "dashboard.insightThroughput" },
] as const satisfies readonly { value: InsightTab; labelKey: string }[];

interface InsightAnalysisCardProps {
	data: InsightData;
	/** 副标题（如「过去 24 小时 · 按上游实际模型统计」）。 */
	subtitle?: string;
	/** 显式桶粒度（由时间窗口推导，透传给折线图 X 轴）。 */
	granularity?: ChartGranularity;
}

/** 性能与可靠性分析卡片：失败诊断 / 延迟分位 / Token 结构 / 吞吐 四主题 Tab。
 *  首页与供应商/虚拟模型/模型详情页共用，按当前页面自动过滤。 */
export function InsightAnalysisCard({ data, subtitle, granularity }: InsightAnalysisCardProps) {
	const { t } = useTranslation();
	const [tab, setTab] = useState<InsightTab>("failure");
	const options = INSIGHT_TABS.map((option) => ({
		value: option.value,
		label: t(option.labelKey),
	}));

	// 窗口内是否有请求：apiKeyRank 来自全量 GROUP BY api_key_name，无请求必为空。
	// 失败/Token 趋势是零填充的，不能用「全 0」判定空数据（健康日零失败会被误判为空）。
	const hasTraffic = data.apiKeyRank.length > 0;
	const noFailureData = !hasTraffic;
	const noLatencyData = !hasTraffic || data.ttftPercentiles.length === 0;
	const noTokenData = !hasTraffic;

	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{t("dashboard.insightTitle")}</CardTitle>
					<p className="text-xs text-muted-foreground">
						{subtitle ?? t("overview.last24HoursByUpstream")}
					</p>
				</div>
				<SegmentedControl options={options} value={tab} onChange={setTab} />
			</CardHeader>
			<CardContent className="space-y-6">
				{tab === "failure" &&
					(noFailureData ? (
						<EmptyState title={t("dashboard.insightNoData")} />
					) : (
						<>
							<FailureTrendChart
								callTrend={data.failureTrend}
								failureTrend={data.failureTrend}
								failureRateTrend={data.failureRateTrend}
								granularity={granularity}
							/>
							{data.failureReasons.length > 0 ? (
								<FailureReasonBarChart
									reasons={data.failureReasons}
									noReasonLabel={t("dashboard.noReason")}
								/>
							) : null}
						</>
					))}
				{tab === "latency" &&
					(noLatencyData ? (
						<EmptyState title={t("dashboard.insightNoData")} />
					) : (
						<>
							<div className="space-y-1">
								<p className="text-sm font-medium">{t("dashboard.ttftPercentiles")}</p>
								<PercentileLineChart data={data.ttftPercentiles} granularity={granularity} />
							</div>
							<div className="space-y-1">
								<p className="text-sm font-medium">{t("dashboard.latencyPercentiles")}</p>
								<PercentileLineChart data={data.latencyPercentiles} granularity={granularity} />
							</div>
						</>
					))}
				{tab === "token" &&
					(noTokenData ? (
						<EmptyState title={t("dashboard.insightNoData")} />
					) : (
						<>
							<TokenStructureChart
								inputTokenTrend={data.inputTokenTrend}
								outputTokenTrend={data.outputTokenTrend}
								cacheHitRateTrend={data.cacheHitRateTrend}
								granularity={granularity}
								formatValue={formatTokenCount}
							/>
							<div className="space-y-1">
								<p className="text-sm font-medium">{t("dashboard.outputPerSec")}</p>
								<OutputPerSecLineChart
									data={data.outputTokensPerSecTrend}
									granularity={granularity}
								/>
							</div>
						</>
					))}
				{tab === "throughput" &&
					(data.rpmTrend.length === 0 ? (
						<EmptyState title={t("dashboard.hourlyThroughputOnly")} />
					) : (
						<ThroughputChart
							rpmTrend={data.rpmTrend}
							tpmTrend={data.tpmTrend}
							streamRatioTrend={data.streamRatioTrend}
							granularity={granularity}
							formatValue={formatTokenCount}
						/>
					))}
			</CardContent>
		</Card>
	);
}
