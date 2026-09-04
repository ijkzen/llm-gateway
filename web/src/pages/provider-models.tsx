import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { AddProviderModelsDialog } from "@/components/provider-models/AddProviderModelsDialog";
import { ProviderModelDetailDialog } from "@/components/provider-models/ProviderModelDetailDialog";
import { ProviderModelSection } from "@/components/provider-models/ProviderModelSection";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { type ProviderModel, useProviderModels } from "@/hooks/use-provider-models";
import { type Provider, useProviders } from "@/hooks/use-providers";
import { PROVIDER_MODELS_PAGE } from "@/lib/pages";
import { RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

interface SelectedModel {
	provider: Provider;
	model: ProviderModel;
}

interface SearchGroup {
	provider: Provider;
	models: ProviderModel[];
}

export default function ProviderModelsPage() {
	const { t } = useTranslation();
	const [addingProvider, setAddingProvider] = useState<Provider | null>(null);
	const [selectedModel, setSelectedModel] = useState<SelectedModel | null>(null);
	const [search, setSearch] = useState("");
	const [searchOpen, setSearchOpen] = useState(false);
	const searchRef = useRef<HTMLDivElement>(null);

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

	const searchGroups = useMemo<SearchGroup[]>(() => {
		const query = search.trim().toLowerCase();
		if (!query || !providers || !models) return [];

		return providers.flatMap((provider) => {
			const matchingModels = models.filter(
				(model) =>
					model.providerId === provider.id && model.providerModelId.toLowerCase().includes(query),
			);
			return matchingModels.length > 0 ? [{ provider, models: matchingModels }] : [];
		});
	}, [models, providers, search]);

	useEffect(() => {
		const closeSearch = (event: PointerEvent) => {
			if (selectedModel || searchRef.current?.contains(event.target as Node)) return;
			setSearchOpen(false);
		};
		document.addEventListener("pointerdown", closeSearch);
		return () => document.removeEventListener("pointerdown", closeSearch);
	}, [selectedModel]);

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
				<div ref={searchRef} className="relative w-full sm:w-72">
					<Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
					<Input
						type="search"
						value={search}
						onChange={(event) => {
							setSearch(event.target.value);
							setSearchOpen(true);
						}}
						onFocus={() => setSearchOpen(true)}
						placeholder={t("providerModels.search")}
						aria-label={t("providerModels.search")}
						className="pl-9"
					/>
					{searchOpen && search.trim() && (
						<div className="absolute right-0 top-full z-20 mt-2 max-h-80 w-full overflow-y-auto rounded-xl border bg-popover p-2 shadow-lg">
							{searchGroups.length === 0 ? (
								<p className="px-3 py-2 text-sm text-muted-foreground">
									{t("providerModels.searchNoResults")}
								</p>
							) : (
								<div className="space-y-3" data-testid="provider-model-search-results">
									{searchGroups.map(({ provider, models: matchingModels }) => (
										<div
											key={provider.id}
											data-testid={`provider-model-search-group-${provider.id}`}
										>
											<p className="px-3 py-1 text-xs font-medium text-muted-foreground">
												{provider.name}
											</p>
											{matchingModels.map((model) => (
												<button
													key={model.modelId}
													type="button"
													onClick={() => setSelectedModel({ provider, model })}
													className="flex w-full rounded-lg px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
												>
													{model.providerModelId}
												</button>
											))}
										</div>
									))}
								</div>
							)}
						</div>
					)}
				</div>
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
						models={(models ?? []).filter((model) => model.providerId === provider.id)}
						onAdd={setAddingProvider}
						onOpenModel={(selectedProvider, model) =>
							setSelectedModel({ provider: selectedProvider, model })
						}
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
				providerProxyAddr={selectedModel?.provider.proxyAddr}
				providerProtocolType={selectedModel?.provider.protocolType ?? 0}
				model={selectedModel?.model ?? null}
			/>
		</div>
	);
}
