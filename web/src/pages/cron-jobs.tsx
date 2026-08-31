import { CronJobDeleteDialog } from "@/components/cron-jobs/CronJobDeleteDialog";
import { CronJobDetail } from "@/components/cron-jobs/CronJobDetail";
import { CronJobEditDialog } from "@/components/cron-jobs/CronJobEditDialog";
import { CronJobList } from "@/components/cron-jobs/CronJobList";
import { CronJobLogsDialog } from "@/components/cron-jobs/CronJobLogsDialog";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type CronJob, useCronJobs } from "@/hooks/use-cron-jobs";
import { CRON_JOBS_PAGE } from "@/lib/pages";
import { RefreshCw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export default function CronJobsPage() {
	const { t } = useTranslation();
	const [selectedName, setSelectedName] = useState<string | null>(null);
	// 默认选中第一个定时任务（数据加载后尚未手动选择时）。
	const [hasUserSelected, setHasUserSelected] = useState(false);
	const [editingJob, setEditingJob] = useState<CronJob | null>(null);
	const [deletingJobName, setDeletingJobName] = useState<string | null>(null);
	const [viewingLogsJob, setViewingLogsJob] = useState<CronJob | null>(null);

	const { data: jobs, isLoading, isError, refetch } = useCronJobs();

	const effectiveSelectedName = hasUserSelected ? selectedName : (jobs?.[0]?.name ?? null);
	const selectedJob = jobs?.find((j) => j.name === effectiveSelectedName) ?? undefined;

	if (isLoading) {
		return (
			<div className="flex h-full min-h-0 flex-col space-y-6">
				<PageHeaderSkeleton />
				<div className="grid flex-1 grid-cols-1 gap-6 lg:grid-cols-3">
					<div className="space-y-4">
						<Skeleton className="h-24 w-full" />
						<Skeleton className="h-24 w-full" />
						<Skeleton className="h-24 w-full" />
					</div>
					<div className="lg:col-span-2">
						<Skeleton className="h-full w-full" />
					</div>
				</div>
			</div>
		);
	}

	if (isError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={CRON_JOBS_PAGE.icon} title={t(CRON_JOBS_PAGE.titleKey)} />
				<ErrorState description={t("cronJobs.errorDescription")} onRetry={() => refetch()} />
			</div>
		);
	}

	return (
		<div className="flex h-full min-h-0 flex-col space-y-6">
			<PageHeader icon={CRON_JOBS_PAGE.icon} title={t(CRON_JOBS_PAGE.titleKey)}>
				<Button variant="outline" size="sm" onClick={() => refetch()}>
					<RefreshCw className="mr-2 size-4" />
					{t("common.refresh")}
				</Button>
			</PageHeader>

			<div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-1 gap-6 lg:grid-cols-3">
				<div className="h-full min-h-0 overflow-auto lg:col-span-1">
					<CronJobList
						jobs={jobs}
						selectedName={effectiveSelectedName}
						onSelect={(job) => {
							setSelectedName(job.name);
							setHasUserSelected(true);
						}}
					/>
				</div>
				<div className="h-full min-h-0 lg:col-span-2">
					<CronJobDetail
						job={selectedJob}
						onEdit={setEditingJob}
						onDelete={setDeletingJobName}
						onViewLogs={setViewingLogsJob}
					/>
				</div>
			</div>

			<CronJobEditDialog
				job={editingJob}
				open={!!editingJob}
				onOpenChange={(open) => !open && setEditingJob(null)}
			/>

			<CronJobDeleteDialog
				jobName={deletingJobName}
				open={!!deletingJobName}
				onOpenChange={(open) => !open && setDeletingJobName(null)}
			/>

			<CronJobLogsDialog
				job={viewingLogsJob}
				open={!!viewingLogsJob}
				onOpenChange={(open) => !open && setViewingLogsJob(null)}
			/>
		</div>
	);
}
