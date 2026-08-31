import { ConfirmDialog } from "@/components/confirm-dialog";
import type { ApiKey } from "@/hooks/use-api-keys";
import { useDeleteApiKey } from "@/hooks/use-api-keys";
import { useToastActions } from "@/hooks/use-toast";
import { useTranslation } from "react-i18next";

interface ApiKeyDeleteDialogProps {
	apiKey: ApiKey | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function ApiKeyDeleteDialog({ apiKey, open, onOpenChange }: ApiKeyDeleteDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const deleteApiKey = useDeleteApiKey();

	const handleConfirm = () => {
		if (!apiKey) return;
		deleteApiKey.mutate(apiKey.id, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess(t("common.deleteSuccess"));
			},
			onError: (error) => {
				toastError(t("common.deleteFailed"), error);
			},
		});
	};

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={onOpenChange}
			title={t("apiKeys.deleteTitle")}
			desc={
				<>
					{t("apiKeys.deleteDesc")} <span className="font-semibold">{apiKey?.name}</span>{" "}
					{t("apiKeys.deleteDescSuffix")}
				</>
			}
			confirmText={t("common.delete")}
			destructive
			isLoading={deleteApiKey.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
