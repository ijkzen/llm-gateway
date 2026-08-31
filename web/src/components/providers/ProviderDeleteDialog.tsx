import { ConfirmDialog } from "@/components/confirm-dialog";
import type { Provider } from "@/hooks/use-providers";
import { useDeleteProvider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { useTranslation } from "react-i18next";

interface ProviderDeleteDialogProps {
	provider: Provider | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function ProviderDeleteDialog({ provider, open, onOpenChange }: ProviderDeleteDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const deleteProvider = useDeleteProvider();

	const handleConfirm = () => {
		if (!provider) return;
		deleteProvider.mutate(provider.id, {
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
			title={t("providers.deleteProviderTitle")}
			desc={
				<>
					{t("providers.deleteProviderDesc")}{" "}
					<span className="font-semibold">{provider?.name}</span>{" "}
					{t("providers.deleteProviderDescSuffix")}
				</>
			}
			confirmText={t("common.delete")}
			destructive
			isLoading={deleteProvider.isPending}
			handleConfirm={handleConfirm}
		/>
	);
}
