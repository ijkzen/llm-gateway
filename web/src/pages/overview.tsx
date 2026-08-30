import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { ProviderModelRaceSection } from "@/components/provider-model-race/ProviderModelRaceSection";
import { ProviderRaceSection } from "@/components/provider-race/ProviderRaceSection";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelRaceSection } from "@/components/virtual-model-race/VirtualModelRaceSection";
import { useDashboardCharts, useDashboardSummary } from "@/hooks/use-dashboard-stats";
import { OVERVIEW_PAGE } from "@/lib/pages";
import { formatTokenCount } from "@/lib/utils";
import { ChartLine, CircleCheck, Coins, DatabaseZap, ListChecks } from "lucide-react";

function formatPercent(rate: number): string {
	// 先对原始比率（0~1）截断保留 5 位小数，再转百分比展示：
	// 如 0.1234567 → 0.12345 → 12.345%；避免 99.789% 被舍入成 100%。
	const truncated = Math.floor(rate * 100_000 + 1e-6);
	return `${truncated / 1000}%`;
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
			<ProviderRaceSection />
			<VirtualModelRaceSection />
			<ProviderModelRaceSection />
		</div>
	);
}
