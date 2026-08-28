import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { RelativeTime } from "@/components/relative-time";
import { StatsCard } from "@/components/stats-card";
import { StatsCardsSkeleton } from "@/components/stats-cards-skeleton";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { useCronJobs } from "@/hooks/use-cron-jobs";
import { useCronStats } from "@/hooks/use-cron-stats";
import { type Setting, useSettings } from "@/hooks/use-settings";
import { OVERVIEW_PAGE } from "@/lib/pages";
import { Clock, Play, Settings } from "lucide-react";
import { useMemo } from "react";

function useRecentSettings(settings: Setting[] | undefined) {
	return useMemo(() => {
		return [...(settings ?? [])]
			.filter((s) => s.updated_at && !Number.isNaN(new Date(s.updated_at).getTime()))
			.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
			.slice(0, 5);
	}, [settings]);
}

export default function OverviewPage() {
	const { isLoading: jobsLoading, isError: jobsError, refetch: refetchJobs } = useCronJobs();
	const {
		data: settings,
		isLoading: settingsLoading,
		isError: settingsError,
		refetch: refetchSettings,
	} = useSettings();

	const stats = useCronStats();
	const recentSettings = useRecentSettings(settings);
	const isLoading = jobsLoading || settingsLoading;
	const isError = jobsError || settingsError;

	if (isLoading) {
		return (
			<div className="space-y-6">
				<PageHeaderSkeleton />
				<StatsCardsSkeleton count={3} />
			</div>
		);
	}

	if (isError) {
		return (
			<div className="space-y-6">
				<PageHeader title={OVERVIEW_PAGE.title} />
				<ErrorState
					description="无法获取总览数据，请检查网络或稍后重试。"
					onRetry={() => {
						refetchJobs();
						refetchSettings();
					}}
				/>
			</div>
		);
	}

	return (
		<div className="space-y-6">
			<PageHeader title={OVERVIEW_PAGE.title} />

			<div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
				<StatsCard icon={Clock} label="定时任务总数" value={stats.total} subLabel="全部任务" />
				<StatsCard icon={Play} label="已启用任务" value={stats.enabled} subLabel="已启用" />
				<StatsCard
					icon={Settings}
					label="系统设置项"
					value={settings?.length ?? 0}
					subLabel="配置项"
				/>
			</div>

			<Card>
				<CardHeader>
					<CardTitle>最近更新的设置</CardTitle>
				</CardHeader>
				<CardContent>
					{recentSettings.length > 0 ? (
						<ul className="space-y-3">
							{recentSettings.map((setting) => (
								<li key={setting.key} className="flex items-center justify-between text-sm">
									<div className="min-w-0">
										<p className="font-medium">{setting.key}</p>
										<p className="truncate text-xs text-muted-foreground">{setting.value}</p>
									</div>
									<RelativeTime date={setting.updated_at} />
								</li>
							))}
						</ul>
					) : (
						<EmptyState title="暂无设置更新" />
					)}
				</CardContent>
			</Card>
		</div>
	);
}
