import { ConfirmDialog } from "@/components/confirm-dialog";
import { useToastActions } from "@/hooks/use-toast";
import { type VirtualModel, useDeleteVirtualModel } from "@/hooks/use-virtual-models";

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
	const { toastSuccess, toastError } = useToastActions();
	const deleteModel = useDeleteVirtualModel();

	const handleConfirm = () => {
		if (!virtualModel) return;
		deleteModel.mutate(virtualModel.virtualModelId, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess("删除成功");
			},
			onError: (error) => toastError("删除失败", error),
		});
	};

	if (!virtualModel) return null;

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={onOpenChange}
			title="删除虚拟模型"
			desc={
				<>
					确定要删除虚拟模型 <span className="font-semibold">{virtualModel.displayId}</span>{" "}
					吗？其成员模型将被释放，此操作无法撤销。
				</>
			}
			confirmText="删除"
			destructive
			isLoading={deleteModel.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
