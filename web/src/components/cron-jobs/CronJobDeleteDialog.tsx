import { ConfirmDialog } from "@/components/confirm-dialog";
import { useDeleteCronJob } from "@/hooks/use-cron-jobs";
import { useToastActions } from "@/hooks/use-toast";

interface CronJobDeleteDialogProps {
	jobName: string | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function CronJobDeleteDialog({ jobName, open, onOpenChange }: CronJobDeleteDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const deleteCronJob = useDeleteCronJob();

	const handleConfirm = () => {
		if (!jobName) return;
		deleteCronJob.mutate(jobName, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess("删除成功");
			},
			onError: (error) => {
				onOpenChange(false);
				toastError("删除失败", error);
			},
		});
	};

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={onOpenChange}
			title="确认删除"
			desc={
				<>
					确定要删除任务 <span className="font-medium">{jobName}</span> 吗？此操作无法撤销。
				</>
			}
			confirmText="删除"
			destructive
			isLoading={deleteCronJob.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
