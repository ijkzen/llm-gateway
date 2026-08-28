import { ConfirmDialog } from "@/components/confirm-dialog";
import type { Provider } from "@/hooks/use-providers";
import { useDeleteProvider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";

interface ProviderDeleteDialogProps {
	provider: Provider | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function ProviderDeleteDialog({ provider, open, onOpenChange }: ProviderDeleteDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const deleteProvider = useDeleteProvider();

	const handleConfirm = () => {
		if (!provider) return;
		deleteProvider.mutate(provider.id, {
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
			title="删除 Provider"
			desc={
				<>
					确定要删除 Provider <span className="font-semibold">{provider?.name}</span> 吗？
					此操作无法撤销。
				</>
			}
			confirmText="删除"
			destructive
			isLoading={deleteProvider.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
