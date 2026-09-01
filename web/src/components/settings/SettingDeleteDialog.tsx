import { ConfirmDialog } from "@/components/confirm-dialog";
import { type Setting, useDeleteSetting } from "@/hooks/use-settings";
import { useToastActions } from "@/hooks/use-toast";

interface SettingDeleteDialogProps {
	setting: Setting | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 删除设置二次确认弹窗：展示将被删除的 key，确认后调用删除接口。 */
export function SettingDeleteDialog({ setting, open, onOpenChange }: SettingDeleteDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const deleteSetting = useDeleteSetting();

	const handleConfirm = () => {
		if (!setting) return;
		deleteSetting.mutate(setting.key, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess("删除成功");
			},
			onError: (error) => {
				toastError("删除失败", error);
			},
		});
	};

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={onOpenChange}
			title="删除设置"
			desc={
				<>
					确定要删除设置项 <span className="font-semibold">{setting?.key}</span>{" "}
					吗？此操作无法撤销。
				</>
			}
			confirmText="删除"
			destructive
			isLoading={deleteSetting.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
