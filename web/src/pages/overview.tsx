import { ModelPieChart, ModelRankBarChart, TrendLineChart } from "@/components/dashboard-charts";
import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { SegmentedControl } from "@/components/segmented-control";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
	type DashboardCharts,
	useDashboardCharts,
	useDashboardSummary,
} from "@/hooks/use-dashboard-stats";
import { OVERVIEW_PAGE } from "@/lib/pages";
import { formatTokenCount } from "@/lib/utils";
import { ChartLine, CircleCheck, Coins, DatabaseZap, ListChecks } from "lucide-react";
import { useState } from "react";

function formatPercent(rate: number): string {
	// 如实展示（保留最多 5 位小数，去尾零），避免 99.789% 被舍入成 100%。
	return `${Number.parseFloat((rate * 100).toFixed(5))}%`;
}

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

function CallAnalysisCard({ charts }: { charts: DashboardCharts }) {
	const [view, setView] = useState<CallView>("trend");
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>调用分析</CardTitle>
					<p className="text-xs text-muted-foreground">过去 24 小时 · 按上游实际模型统计</p>
				</div>
				<SegmentedControl options={CALL_VIEW_OPTIONS} value={view} onChange={setView} />
			</CardHeader>
			<CardContent>
				{view === "trend" && <TrendLineChart data={charts.callTrend} label="调用次数" />}
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

function TokenAnalysisCard({ charts }: { charts: DashboardCharts }) {
	const [view, setView] = useState<TokenView>("trend");
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>Token 分析</CardTitle>
					<p className="text-xs text-muted-foreground">过去 24 小时 · 按上游实际模型统计</p>
				</div>
				<SegmentedControl options={TOKEN_VIEW_OPTIONS} value={view} onChange={setView} />
			</CardHeader>
			<CardContent>
				{view === "trend" && (
					<TrendLineChart
						data={charts.tokenTrend}
						label="Token 数"
						formatValue={formatTokenCount}
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

export default function OverviewPage() {
	const summaryQuery = useDashboardSummary();
	const chartsQuery = useDashboardCharts();

	const isLoading = summaryQuery.isLoading || chartsQuery.isLoading;
	const isError = summaryQuery.isError || chartsQuery.isError;

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<StatsCardsSkeleton count={4} />
				<Card>
					<CardHeader>
						<Skeleton className="h-5 w-24" />
					</CardHeader>
					<CardContent>
						<Skeleton className="h-[260px] w-full" />
					</CardContent>
				</Card>
			</div>
		);
	}

	if (isError || !summaryQuery.data || !chartsQuery.data) {
		return (
			<div className="space-y-6">
				<PageHeader title={OVERVIEW_PAGE.title} icon={ChartLine} />
				<ErrorState
					description="无法获取数据面板数据，请检查网络或稍后重试。"
					onRetry={() => {
						summaryQuery.refetch();
						chartsQuery.refetch();
					}}
				/>
			</div>
		);
	}

	const summary = summaryQuery.data;

	return (
		<div className="space-y-6">
			<PageHeader title={OVERVIEW_PAGE.title} icon={ChartLine} />

			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
				<StatsCard
					icon={ListChecks}
					label="累计请求数"
					value={summary.totalRequests.toLocaleString()}
					subLabel="全部历史"
				/>
				<StatsCard
					icon={CircleCheck}
					label="请求成功率"
					value={formatPercent(summary.successRate)}
					subLabel="全部历史"
				/>
				<StatsCard
					icon={Coins}
					label="总计 Token"
					value={formatTokenCount(summary.totalTokens)}
					subLabel="输入 + 输出"
				/>
				<StatsCard
					icon={DatabaseZap}
					label="缓存命中率"
					value={formatPercent(summary.cacheHitRate)}
					subLabel="缓存 token / 输入 token"
				/>
			</div>

			<CallAnalysisCard charts={chartsQuery.data} />
			<TokenAnalysisCard charts={chartsQuery.data} />
		</div>
	);
}
