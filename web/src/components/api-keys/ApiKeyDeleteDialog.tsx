import { ConfirmDialog } from "@/components/confirm-dialog";
import type { ApiKey } from "@/hooks/use-api-keys";
import { useDeleteApiKey } from "@/hooks/use-api-keys";
import { useToastActions } from "@/hooks/use-toast";

interface ApiKeyDeleteDialogProps {
	apiKey: ApiKey | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function ApiKeyDeleteDialog({ apiKey, open, onOpenChange }: ApiKeyDeleteDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const deleteApiKey = useDeleteApiKey();

	const handleConfirm = () => {
		if (!apiKey) return;
		deleteApiKey.mutate(apiKey.id, {
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
			title="删除 API Key"
			desc={
				<>
					确定要删除 API Key <span className="font-semibold">{apiKey?.name}</span> 吗？此操作无法
					撤销，使用该 Key 的调用方将立即失效。
				</>
			}
			confirmText="删除"
			destructive
			isLoading={deleteApiKey.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
