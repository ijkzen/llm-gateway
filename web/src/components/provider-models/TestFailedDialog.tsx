import { ConfirmDialog } from "@/components/confirm-dialog";
import { useTranslation } from "react-i18next";

interface TestFailedDialogProps {
	message: string | null;
	onClose: () => void;
}

/** 模型/测速测试失败详情弹窗：展示后端返回的人类可读错误，单「关闭」按钮。 */
export function TestFailedDialog({ message, onClose }: TestFailedDialogProps) {
	const { t } = useTranslation();
	return (
		<ConfirmDialog
			open={message !== null}
			onOpenChange={(open) => {
				if (!open) onClose();
			}}
			title={t("providerModels.testFailedTitle")}
			desc={
				<>
					<p>{t("providerModels.testFailedDesc")}</p>
					<p className="mt-2 break-all rounded-lg bg-muted p-3 font-mono text-xs text-destructive">
						{message}
					</p>
				</>
			}
			confirmText={t("providerModels.close")}
			handleConfirm={onClose}
		/>
	);
}
