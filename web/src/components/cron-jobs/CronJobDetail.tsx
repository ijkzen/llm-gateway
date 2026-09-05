import { EmptyState } from "@/components/empty-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Switch } from "@/components/ui/switch";
import type { CronJob } from "@/hooks/use-cron-jobs";
import { useRunCronJob, useUpdateCronJob } from "@/hooks/use-cron-jobs";
import { useToastActions } from "@/hooks/use-toast";
import { DEFAULT_GROUP } from "@/lib/constants";
import { formatDateTime } from "@/lib/utils";
import { MoreHorizontal, Pencil, Play, ScrollText, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface CronJobDetailProps {
	job: CronJob | undefined;
	onEdit: (job: CronJob) => void;
	onDelete: (name: string) => void;
	onViewLogs: (job: CronJob) => void;
}

export function CronJobDetail({ job, onEdit, onDelete, onViewLogs }: CronJobDetailProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateCronJob = useUpdateCronJob();
	const runCronJob = useRunCronJob();

	if (!job) {
		return (
			<EmptyState
				title={t("cronJobs.noJobSelected")}
				description={t("cronJobs.noJobSelectedHint")}
			/>
		);
	}

	return (
		<Card className="flex flex-1 flex-col">
			<CardHeader className="border-b">
				<div className="flex items-start justify-between gap-4">
					<div>
						<CardTitle className="text-xl">{job.name}</CardTitle>
						<p className="mt-1 text-sm text-muted-foreground">{job.title}</p>
					</div>
					<Switch
						checked={job.enabled}
						disabled={updateCronJob.isPending}
						aria-label={`${t("cronJobs.toggleStatus")} ${job.name} ${t("cronJobs.toggleStatusSuffix")}`}
						onCheckedChange={() =>
							updateCronJob.mutate(
								{ name: job.name, enabled: !job.enabled },
								{
									onSuccess: () => toastSuccess(t("common.success")),
									onError: (error) => toastError(t("common.error"), error),
								},
							)
						}
					/>
				</div>
			</CardHeader>
			<CardContent className="flex-1 space-y-6 py-6">
				<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("cronJobs.expression")}
						</p>
						<Badge variant="outline" className="mt-1 font-mono text-xs">
							{job.expression}
						</Badge>
					</div>
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("cronJobs.group")}
						</p>
						<p className="mt-1 text-sm">{job.group || DEFAULT_GROUP}</p>
					</div>
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("cronJobs.lastRunAt")}
						</p>
						<p className="mt-1 text-sm">{formatDateTime(job.last_run_at)}</p>
					</div>
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("cronJobs.nextRunAt")}
						</p>
						<p className="mt-1 text-sm">{formatDateTime(job.next_run_at)}</p>
					</div>
				</div>

				{job.description && (
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							{t("cronJobs.description")}
						</p>
						<p className="mt-1 text-sm text-muted-foreground">{job.description}</p>
					</div>
				)}

				<div className="flex items-center gap-2 pt-4">
					<Button
						variant="outline"
						size="sm"
						onClick={() =>
							runCronJob.mutate(job.name, {
								onSuccess: () => toastSuccess(t("cronJobs.runTriggered")),
								onError: (error) => toastError(t("cronJobs.runFailed"), error),
							})
						}
					>
						<Play className="mr-2 size-4" />
						{t("cronJobs.runNow")}
					</Button>
					<DropdownMenu modal={false}>
						<DropdownMenuTrigger asChild>
							<Button
								variant="outline"
								size="icon"
								className="size-9"
								aria-label={t("common.moreActions")}
							>
								<MoreHorizontal className="size-4" />
							</Button>
						</DropdownMenuTrigger>
						<DropdownMenuContent align="end">
							<DropdownMenuItem onClick={() => onEdit(job)}>
								<Pencil className="size-4" />
								{t("cronJobs.edit")}
							</DropdownMenuItem>
							<DropdownMenuSeparator />
							<DropdownMenuItem variant="destructive" onClick={() => onDelete(job.name)}>
								<Trash2 className="size-4" />
								{t("cronJobs.delete")}
							</DropdownMenuItem>
						</DropdownMenuContent>
					</DropdownMenu>
					{/* 日志入口固定在操作区右下角 */}
					<Button variant="outline" size="sm" className="ml-auto" onClick={() => onViewLogs(job)}>
						<ScrollText className="mr-2 size-4" />
						{t("common.viewLogs")}
					</Button>
				</div>
			</CardContent>
		</Card>
	);
}
