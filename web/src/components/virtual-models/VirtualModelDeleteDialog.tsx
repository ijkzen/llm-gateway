import { ConfirmDialog } from "@/components/confirm-dialog";
import { useToastActions } from "@/hooks/use-toast";
import { type VirtualModel, useDeleteVirtualModel } from "@/hooks/use-virtual-models";
import { useTranslation } from "react-i18next";

interface VirtualModelDeleteDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	virtualModel: VirtualModel | null;
}

/** 删除虚拟模型二次确认弹窗：删除后成员模型被释放，可再映射到其他虚拟模型。 */
export function VirtualModelDeleteDialog({
	open,
	onOpenChange,
	virtualModel,
}: VirtualModelDeleteDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const deleteModel = useDeleteVirtualModel();

	const handleConfirm = () => {
		if (!virtualModel) return;
		deleteModel.mutate(virtualModel.virtualModelId, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess(t("common.deleteSuccess"));
			},
			onError: (error) => toastError(t("common.deleteFailed"), error),
		});
	};

	if (!virtualModel) return null;

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={onOpenChange}
			title={t("virtualModels.deleteTitle")}
			desc={
				<>
					{t("virtualModels.deleteDesc")}{" "}
					<span className="font-semibold">{virtualModel.displayId}</span>{" "}
					{t("virtualModels.deleteDescSuffix")}
				</>
			}
			confirmText={t("common.delete")}
			destructive
			isLoading={deleteModel.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
