import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { AddProviderModelsDialog } from "@/components/provider-models/AddProviderModelsDialog";
import { ProviderModelDetailDialog } from "@/components/provider-models/ProviderModelDetailDialog";
import { ProviderModelSection } from "@/components/provider-models/ProviderModelSection";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type ProviderModel, useProviderModels } from "@/hooks/use-provider-models";
import { type Provider, useProviders } from "@/hooks/use-providers";
import { PROVIDER_MODELS_PAGE } from "@/lib/pages";
import { RefreshCw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

interface SelectedModel {
	provider: Provider;
	model: ProviderModel;
}

export default function ProviderModelsPage() {
	const { t } = useTranslation();
	const [addingProvider, setAddingProvider] = useState<Provider | null>(null);
	const [selectedModel, setSelectedModel] = useState<SelectedModel | null>(null);

	const {
		data: providers,
		isLoading: providersLoading,
		isError: providersError,
		refetch: refetchProviders,
	} = useProviders();
	const {
		data: models,
		isLoading: modelsLoading,
		isError: modelsError,
		refetch: refetchModels,
	} = useProviderModels();

	if (providersLoading || modelsLoading) {
		return (
			<div className="flex h-full min-h-0 flex-col space-y-6">
				<PageHeaderSkeleton />
				<div className="space-y-6">
					<Skeleton className="h-16 w-full" />
					<Skeleton className="h-16 w-full" />
					<Skeleton className="h-16 w-full" />
				</div>
			</div>
		);
	}

	if (providersError || modelsError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={PROVIDER_MODELS_PAGE.icon} title={t(PROVIDER_MODELS_PAGE.titleKey)} />
				<ErrorState
					description={t("providerModels.errorDescription")}
					onRetry={() => {
						refetchProviders();
						refetchModels();
					}}
				/>
			</div>
		);
	}

	if (!providers || providers.length === 0) {
		return (
			<div className="flex h-full min-h-0 flex-col space-y-6">
				<PageHeader icon={PROVIDER_MODELS_PAGE.icon} title={t(PROVIDER_MODELS_PAGE.titleKey)} />
				<EmptyState
					title={t("providerModels.emptyTitle")}
					description={t("providerModels.emptyHint")}
					action={
						<Button asChild variant="outline" size="sm">
							<Link to="/providers">{t("providerModels.goCreateProvider")}</Link>
						</Button>
					}
				/>
			</div>
		);
	}

	return (
		<div className="flex h-full min-h-0 flex-col space-y-6 overflow-auto">
			<PageHeader icon={PROVIDER_MODELS_PAGE.icon} title={t(PROVIDER_MODELS_PAGE.titleKey)}>
				<Button
					variant="outline"
					size="sm"
					onClick={() => {
						refetchProviders();
						refetchModels();
					}}
				>
					<RefreshCw className="mr-2 size-4" />
					{t("common.refresh")}
				</Button>
			</PageHeader>

			<div className="space-y-6 pb-6">
				{providers.map((provider) => (
					<ProviderModelSection
						key={provider.id}
						provider={provider}
						models={(models ?? []).filter((m) => m.providerId === provider.id)}
						onAdd={setAddingProvider}
						onOpenModel={(p, m) => setSelectedModel({ provider: p, model: m })}
					/>
				))}
			</div>

			<AddProviderModelsDialog
				open={addingProvider !== null}
				onOpenChange={(open) => {
					if (!open) setAddingProvider(null);
				}}
				provider={addingProvider}
			/>

			<ProviderModelDetailDialog
				open={selectedModel !== null}
				onOpenChange={(open) => {
					if (!open) setSelectedModel(null);
				}}
				providerId={selectedModel?.provider.id ?? 0}
				providerName={selectedModel?.provider.name ?? ""}
				// 只读态展示「继承供应商代理」时需要供应商代理地址。
				providerProxyAddr={selectedModel?.provider.proxyAddr}
				model={selectedModel?.model ?? null}
			/>
		</div>
	);
}
