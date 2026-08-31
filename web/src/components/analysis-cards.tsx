import { ModelPieChart, ModelRankBarChart, TrendLineChart } from "@/components/dashboard-charts";
import { EmptyState } from "@/components/empty-state";
import { SegmentedControl } from "@/components/segmented-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import type { ChartGranularity } from "@/lib/race-period";
import { formatTokenCount } from "@/lib/utils";
import { useState } from "react";
import { useTranslation } from "react-i18next";

type CallView = "trend" | "distribution" | "rank";
type TokenView = "trend" | "distribution" | "rank";

const CALL_VIEW_KEYS = [
	{ value: "trend", labelKey: "dashboard.callTrend" },
	{ value: "distribution", labelKey: "dashboard.callDistribution" },
	{ value: "rank", labelKey: "dashboard.callRank" },
] as const satisfies readonly { value: CallView; labelKey: string }[];

const TOKEN_VIEW_KEYS = [
	{ value: "trend", labelKey: "dashboard.tokenTrend" },
	{ value: "distribution", labelKey: "dashboard.tokenDistribution" },
	{ value: "rank", labelKey: "dashboard.tokenRank" },
] as const satisfies readonly { value: TokenView; labelKey: string }[];

interface AnalysisCardProps {
	charts: DashboardCharts;
	/** 副标题（如「过去 24 小时 · 按上游实际模型统计」）。 */
	subtitle?: string;
	/** 显式桶粒度（由时间窗口推导，透传给折线图 X 轴）。 */
	granularity?: ChartGranularity;
}

/** 调用分析卡片：趋势 / 分布 / 排行三态切换。首页与供应商二级页共用。 */
export function CallAnalysisCard({ charts, subtitle, granularity }: AnalysisCardProps) {
	const { t } = useTranslation();
	const [view, setView] = useState<CallView>("trend");
	const callViewOptions = CALL_VIEW_KEYS.map((option) => ({
		value: option.value,
		label: t(option.labelKey),
	}));
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{t("dashboard.analysis")}</CardTitle>
					<p className="text-xs text-muted-foreground">
						{subtitle ?? t("overview.last24HoursByUpstream")}
					</p>
				</div>
				<SegmentedControl options={callViewOptions} value={view} onChange={setView} />
			</CardHeader>
			<CardContent>
				{view === "trend" && (
					<TrendLineChart
						data={charts.callTrend}
						label={t("overview.calls")}
						granularity={granularity}
					/>
				)}
				{view === "distribution" &&
					(charts.callByModel.length > 0 ? (
						<ModelPieChart data={charts.callByModel} />
					) : (
						<EmptyState title={t("dashboard.noCallData")} />
					))}
				{view === "rank" &&
					(charts.callByModel.length > 0 ? (
						<ModelRankBarChart data={charts.callByModel} />
					) : (
						<EmptyState title={t("dashboard.noCallData")} />
					))}
			</CardContent>
		</Card>
	);
}

/** Token 分析卡片：趋势 / 分布 / 排行三态切换。首页与供应商二级页共用。 */
export function TokenAnalysisCard({ charts, subtitle, granularity }: AnalysisCardProps) {
	const { t } = useTranslation();
	const [view, setView] = useState<TokenView>("trend");
	const tokenViewOptions = TOKEN_VIEW_KEYS.map((option) => ({
		value: option.value,
		label: t(option.labelKey),
	}));
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{t("dashboard.tokenAnalysis")}</CardTitle>
					<p className="text-xs text-muted-foreground">
						{subtitle ?? t("overview.last24HoursByUpstream")}
					</p>
				</div>
				<SegmentedControl options={tokenViewOptions} value={view} onChange={setView} />
			</CardHeader>
			<CardContent>
				{view === "trend" && (
					<TrendLineChart
						data={charts.tokenTrend}
						label={t("overview.tokens")}
						formatValue={formatTokenCount}
						kind="tokens"
						granularity={granularity}
					/>
				)}
				{view === "distribution" &&
					(charts.tokenByModel.length > 0 ? (
						<ModelPieChart
							data={charts.tokenByModel}
							formatValue={formatTokenCount}
							kind="tokens"
						/>
					) : (
						<EmptyState title={t("dashboard.noTokenData")} />
					))}
				{view === "rank" &&
					(charts.tokenByModel.length > 0 ? (
						<ModelRankBarChart
							data={charts.tokenByModel}
							formatValue={formatTokenCount}
							kind="tokens"
						/>
					) : (
						<EmptyState title={t("dashboard.noTokenData")} />
					))}
			</CardContent>
		</Card>
	);
}
