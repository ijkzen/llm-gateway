import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { ProviderDeleteDialog } from "@/components/providers/ProviderDeleteDialog";
import { ProviderDetail } from "@/components/providers/ProviderDetail";
import { ProviderEditDialog } from "@/components/providers/ProviderEditDialog";
import { ProviderList } from "@/components/providers/ProviderList";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { type Provider, useProviders } from "@/hooks/use-providers";
import { PROVIDERS_PAGE } from "@/lib/pages";
import { Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

export default function ProvidersPage() {
	const { t } = useTranslation();
	const [selectedId, setSelectedId] = useState<number | null>(null);
	const [creating, setCreating] = useState(false);
	const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
	const [deletingProvider, setDeletingProvider] = useState<Provider | null>(null);

	const { data: providers, isLoading, isError, refetch } = useProviders();

	// 默认选中第一个供应商（数据加载后尚未手动选择时）。
	const [hasUserSelected, setHasUserSelected] = useState(false);
	const effectiveSelectedId = hasUserSelected ? selectedId : (providers?.[0]?.id ?? null);
	const effectiveProvider = providers?.find((p) => p.id === effectiveSelectedId) ?? undefined;

	if (isLoading) {
		return (
			<div className="flex h-full min-h-0 flex-col space-y-6">
				<PageHeaderSkeleton />
				<div className="grid flex-1 grid-cols-1 gap-6 lg:grid-cols-3">
					<div className="space-y-4">
						<Skeleton className="h-24 w-full" />
						<Skeleton className="h-24 w-full" />
						<Skeleton className="h-24 w-full" />
					</div>
					<div className="lg:col-span-2">
						<Skeleton className="h-full w-full" />
					</div>
				</div>
			</div>
		);
	}

	if (isError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={PROVIDERS_PAGE.icon} title={t(PROVIDERS_PAGE.titleKey)} />
				<ErrorState description={t("providers.errorDescription")} onRetry={() => refetch()} />
			</div>
		);
	}

	return (
		<div className="flex h-full min-h-0 flex-col space-y-6">
			<PageHeader icon={PROVIDERS_PAGE.icon} title={t(PROVIDERS_PAGE.titleKey)}>
				<Button variant="outline" size="sm" onClick={() => refetch()}>
					<RefreshCw className="mr-2 size-4" />
					{t("common.refresh")}
				</Button>
				<Button size="sm" onClick={() => setCreating(true)}>
					<Plus className="mr-2 size-4" />
					{t("providers.create")}
				</Button>
			</PageHeader>

			<div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-1 gap-6 lg:grid-cols-3">
				<div className="h-full min-h-0 overflow-auto lg:col-span-1">
					<ProviderList
						providers={providers}
						selectedId={effectiveSelectedId}
						onSelect={(provider) => {
							setSelectedId(provider.id);
							setHasUserSelected(true);
						}}
					/>
				</div>
				<div className="h-full min-h-0 lg:col-span-2">
					<ProviderDetail
						provider={effectiveProvider}
						onEdit={setEditingProvider}
						onDelete={setDeletingProvider}
					/>
				</div>
			</div>

			<ProviderEditDialog
				open={creating || !!editingProvider}
				onOpenChange={(open) => {
					if (!open) {
						setCreating(false);
						setEditingProvider(null);
					}
				}}
				provider={editingProvider}
			/>

			<ProviderDeleteDialog
				provider={deletingProvider}
				open={!!deletingProvider}
				onOpenChange={(open) => !open && setDeletingProvider(null)}
			/>
		</div>
	);
}
