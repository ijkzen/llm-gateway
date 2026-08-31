import { ConfirmDialog } from "@/components/confirm-dialog";
import { useDeleteCronJob } from "@/hooks/use-cron-jobs";
import { useToastActions } from "@/hooks/use-toast";
import { useTranslation } from "react-i18next";

interface CronJobDeleteDialogProps {
	jobName: string | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function CronJobDeleteDialog({ jobName, open, onOpenChange }: CronJobDeleteDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const deleteCronJob = useDeleteCronJob();

	const handleConfirm = () => {
		if (!jobName) return;
		deleteCronJob.mutate(jobName, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess(t("common.deleteSuccess"));
			},
			onError: (error) => {
				onOpenChange(false);
				toastError(t("common.deleteFailed"), error);
			},
		});
	};

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={onOpenChange}
			title={t("cronJobs.deleteTitle")}
			desc={
				<>
					{t("cronJobs.deleteDesc")} <span className="font-medium">{jobName}</span>{" "}
					{t("cronJobs.deleteDescSuffix")}
				</>
			}
			confirmText={t("common.delete")}
			destructive
			isLoading={deleteCronJob.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
