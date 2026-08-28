import { EmptyState } from "@/components/empty-state";
import { StatusBadge } from "@/components/status-badge";
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
import { MoreHorizontal, Pencil, Play, ScrollText, Trash2 } from "lucide-react";

interface CronJobDetailProps {
	job: CronJob | undefined;
	onEdit: (job: CronJob) => void;
	onDelete: (name: string) => void;
	onViewLogs: (job: CronJob) => void;
}

function formatDate(dateStr: string) {
	if (!dateStr) return "—";
	const ts = new Date(dateStr).getTime();
	if (Number.isNaN(ts) || ts <= 0) return "—";
	return new Date(dateStr).toLocaleString("zh-CN");
}

export function CronJobDetail({ job, onEdit, onDelete, onViewLogs }: CronJobDetailProps) {
	const { toastSuccess, toastError } = useToastActions();
	const updateCronJob = useUpdateCronJob();
	const runCronJob = useRunCronJob();

	if (!job) {
		return <EmptyState title="未选择任务" description="在左侧选择一个定时任务查看详情" />;
	}

	return (
		<Card className="flex flex-1 flex-col">
			<CardHeader className="border-b">
				<div className="flex items-start justify-between gap-4">
					<div>
						<CardTitle className="text-xl">{job.name}</CardTitle>
						<p className="mt-1 text-sm text-muted-foreground">{job.title}</p>
					</div>
					<div className="flex items-center gap-2">
						<Switch
							checked={job.enabled}
							disabled={updateCronJob.isPending}
							aria-label={`切换任务 ${job.name} 状态`}
							onCheckedChange={() =>
								updateCronJob.mutate(
									{ name: job.name, enabled: !job.enabled },
									{
										onSuccess: () => toastSuccess("操作成功"),
										onError: (error) => toastError("操作失败", error),
									},
								)
							}
						/>
						<StatusBadge status={job.enabled ? "enabled" : "disabled"} />
					</div>
				</div>
			</CardHeader>
			<CardContent className="flex-1 space-y-6 py-6">
				<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							表达式
						</p>
						<Badge variant="outline" className="mt-1 font-mono text-xs">
							{job.expression}
						</Badge>
					</div>
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							分组
						</p>
						<p className="mt-1 text-sm">{job.group || DEFAULT_GROUP}</p>
					</div>
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							上次执行
						</p>
						<p className="mt-1 text-sm">{formatDate(job.last_run_at)}</p>
					</div>
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							下次执行
						</p>
						<p className="mt-1 text-sm">{formatDate(job.next_run_at)}</p>
					</div>
				</div>

				{job.description && (
					<div>
						<p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
							描述
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
								onSuccess: () => toastSuccess("任务已触发执行"),
								onError: (error) => toastError("执行失败", error),
							})
						}
					>
						<Play className="mr-2 size-4" />
						立即执行
					</Button>
					<DropdownMenu modal={false}>
						<DropdownMenuTrigger asChild>
							<Button variant="outline" size="icon" className="size-9" aria-label="更多操作">
								<MoreHorizontal className="size-4" />
							</Button>
						</DropdownMenuTrigger>
						<DropdownMenuContent align="end">
							<DropdownMenuItem onClick={() => onEdit(job)}>
								<Pencil className="size-4" />
								编辑
							</DropdownMenuItem>
							<DropdownMenuSeparator />
							<DropdownMenuItem variant="destructive" onClick={() => onDelete(job.name)}>
								<Trash2 className="size-4" />
								删除
							</DropdownMenuItem>
						</DropdownMenuContent>
					</DropdownMenu>
					{/* 日志入口固定在操作区右下角 */}
					<Button variant="outline" size="sm" className="ml-auto" onClick={() => onViewLogs(job)}>
						<ScrollText className="mr-2 size-4" />
						查看日志
					</Button>
				</div>
			</CardContent>
		</Card>
	);
}
