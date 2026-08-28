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

export default function CronJobsPage() {
	const [selectedName, setSelectedName] = useState<string | null>(null);
	const [editingJob, setEditingJob] = useState<CronJob | null>(null);
	const [deletingJobName, setDeletingJobName] = useState<string | null>(null);
	const [viewingLogsJob, setViewingLogsJob] = useState<CronJob | null>(null);

	const { data: jobs, isLoading, isError, refetch } = useCronJobs();

	const selectedJob = jobs?.find((j) => j.name === selectedName) ?? undefined;

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
				<PageHeader
					icon={CRON_JOBS_PAGE.icon}
					title={CRON_JOBS_PAGE.title}
					description={CRON_JOBS_PAGE.description}
				/>
				<ErrorState
					description="无法获取定时任务数据，请检查网络或稍后重试。"
					onRetry={() => refetch()}
				/>
			</div>
		);
	}

	return (
		<div className="flex h-full min-h-0 flex-col space-y-6">
			<PageHeader
				icon={CRON_JOBS_PAGE.icon}
				title={CRON_JOBS_PAGE.title}
				description={CRON_JOBS_PAGE.description}
			>
				<Button variant="outline" size="sm" onClick={() => refetch()}>
					<RefreshCw className="mr-2 size-4" />
					刷新
				</Button>
			</PageHeader>

			<div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-1 gap-6 lg:grid-cols-3">
				<div className="h-full min-h-0 overflow-auto lg:col-span-1">
					<CronJobList
						jobs={jobs}
						selectedName={selectedName}
						onSelect={(job) => setSelectedName(job.name)}
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
