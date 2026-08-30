import { ModelPieChart, ModelRankBarChart, TrendLineChart } from "@/components/dashboard-charts";
import { EmptyState } from "@/components/empty-state";
import { SegmentedControl } from "@/components/segmented-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import type { ChartGranularity } from "@/lib/race-period";
import { formatTokenCount } from "@/lib/utils";
import { useState } from "react";

type CallView = "trend" | "distribution" | "rank";
type TokenView = "trend" | "distribution" | "rank";

const CALL_VIEW_OPTIONS = [
	{ value: "trend", label: "调用趋势" },
	{ value: "distribution", label: "调用次数分布" },
	{ value: "rank", label: "调用次数排行" },
] as const satisfies readonly { value: CallView; label: string }[];

const TOKEN_VIEW_OPTIONS = [
	{ value: "trend", label: "token 使用分布" },
	{ value: "distribution", label: "token 模型分布" },
	{ value: "rank", label: "token 模型排行" },
] as const satisfies readonly { value: TokenView; label: string }[];

interface AnalysisCardProps {
	charts: DashboardCharts;
	/** 副标题（如「过去 24 小时 · 按上游实际模型统计」）。 */
	subtitle?: string;
	/** 显式桶粒度（由时间窗口推导，透传给折线图 X 轴）。 */
	granularity?: ChartGranularity;
}

/** 调用分析卡片：趋势 / 分布 / 排行三态切换。首页与供应商二级页共用。 */
export function CallAnalysisCard({ charts, subtitle, granularity }: AnalysisCardProps) {
	const [view, setView] = useState<CallView>("trend");
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>调用分析</CardTitle>
					<p className="text-xs text-muted-foreground">
						{subtitle ?? "过去 24 小时 · 按上游实际模型统计"}
					</p>
				</div>
				<SegmentedControl options={CALL_VIEW_OPTIONS} value={view} onChange={setView} />
			</CardHeader>
			<CardContent>
				{view === "trend" && (
					<TrendLineChart data={charts.callTrend} label="调用次数" granularity={granularity} />
				)}
				{view === "distribution" &&
					(charts.callByModel.length > 0 ? (
						<ModelPieChart data={charts.callByModel} />
					) : (
						<EmptyState title="暂无调用数据" />
					))}
				{view === "rank" &&
					(charts.callByModel.length > 0 ? (
						<ModelRankBarChart data={charts.callByModel} />
					) : (
						<EmptyState title="暂无调用数据" />
					))}
			</CardContent>
		</Card>
	);
}

/** Token 分析卡片：趋势 / 分布 / 排行三态切换。首页与供应商二级页共用。 */
export function TokenAnalysisCard({ charts, subtitle, granularity }: AnalysisCardProps) {
	const [view, setView] = useState<TokenView>("trend");
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>Token 分析</CardTitle>
					<p className="text-xs text-muted-foreground">
						{subtitle ?? "过去 24 小时 · 按上游实际模型统计"}
					</p>
				</div>
				<SegmentedControl options={TOKEN_VIEW_OPTIONS} value={view} onChange={setView} />
			</CardHeader>
			<CardContent>
				{view === "trend" && (
					<TrendLineChart
						data={charts.tokenTrend}
						label="Token 数"
						formatValue={formatTokenCount}
						granularity={granularity}
					/>
				)}
				{view === "distribution" &&
					(charts.tokenByModel.length > 0 ? (
						<ModelPieChart data={charts.tokenByModel} formatValue={formatTokenCount} />
					) : (
						<EmptyState title="暂无 token 数据" />
					))}
				{view === "rank" &&
					(charts.tokenByModel.length > 0 ? (
						<ModelRankBarChart data={charts.tokenByModel} formatValue={formatTokenCount} />
					) : (
						<EmptyState title="暂无 token 数据" />
					))}
			</CardContent>
		</Card>
	);
}
