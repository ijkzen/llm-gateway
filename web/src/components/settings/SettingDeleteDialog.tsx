import { ConfirmDialog } from "@/components/confirm-dialog";
import { useSettingSubmitCallbacks } from "@/components/settings/use-setting-submit-callbacks";
import { type Setting, useDeleteSetting } from "@/hooks/use-settings";

interface SettingDeleteDialogProps {
	setting: Setting | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 删除设置二次确认弹窗：展示将被删除的 key，确认后调用删除接口。 */
export function SettingDeleteDialog({ setting, open, onOpenChange }: SettingDeleteDialogProps) {
	const deleteSetting = useDeleteSetting();

	// 18-17：成功关窗 + 提示的三处样板收敛到域内 helper。
	const callbacks = useSettingSubmitCallbacks(onOpenChange, "删除成功", "删除失败");

	const handleConfirm = () => {
		if (!setting) return;
		deleteSetting.mutate(setting.key, callbacks);
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
