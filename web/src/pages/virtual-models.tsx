import { ErrorState } from "@/components/error-state";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelDeleteDialog } from "@/components/virtual-models/VirtualModelDeleteDialog";
import { VirtualModelEditDialog } from "@/components/virtual-models/VirtualModelEditDialog";
import { VirtualModelSection } from "@/components/virtual-models/VirtualModelSection";
import { useProviderModels } from "@/hooks/use-provider-models";
import { useProviders } from "@/hooks/use-providers";
import { type VirtualModel, useVirtualModels } from "@/hooks/use-virtual-models";
import { VIRTUAL_MODELS_PAGE } from "@/lib/pages";
import { Plus, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

export default function VirtualModelsPage() {
	const { t } = useTranslation();
	const [creating, setCreating] = useState(false);
	const [editing, setEditing] = useState<VirtualModel | null>(null);
	const [deleting, setDeleting] = useState<VirtualModel | null>(null);

	const { data: virtualModels, isLoading, isError, refetch } = useVirtualModels();
	const {
		data: providers,
		isLoading: providersLoading,
		isError: providersError,
		refetch: refetchProviders,
	} = useProviders();
	const {
		data: providerModels,
		isLoading: modelsLoading,
		isError: modelsError,
		refetch: refetchModels,
	} = useProviderModels();

	// 编辑弹窗中需排除的 modelId：全部已映射成员，但保留当前编辑目标自身的成员。
	const mappedForEdit = useMemo(() => {
		const editingId = editing?.virtualModelId ?? null;
		return new Set(
			(virtualModels ?? [])
				.filter((vm) => vm.virtualModelId !== editingId)
				.flatMap((vm) => vm.items.map((item) => item.modelId)),
		);
	}, [virtualModels, editing]);

	if (isLoading || providersLoading || modelsLoading) {
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

	if (isError || providersError || modelsError) {
		return (
			<div className="space-y-6">
				<PageHeader icon={VIRTUAL_MODELS_PAGE.icon} title={t(VIRTUAL_MODELS_PAGE.titleKey)} />
				<ErrorState
					description={t("virtualModels.errorDescription")}
					onRetry={() => {
						refetch();
						refetchProviders();
						refetchModels();
					}}
				/>
			</div>
		);
	}

	const models = virtualModels ?? [];

	return (
		<div className="flex h-full min-h-0 flex-col space-y-6 overflow-auto">
			<PageHeader icon={VIRTUAL_MODELS_PAGE.icon} title={t(VIRTUAL_MODELS_PAGE.titleKey)}>
				<Button variant="outline" size="sm" onClick={() => refetch()}>
					<RefreshCw className="mr-2 size-4" />
					{t("common.refresh")}
				</Button>
				<Button
					size="sm"
					onClick={() => {
						setCreating(true);
					}}
				>
					<Plus className="mr-2 size-4" />
					{t("virtualModels.add")}
				</Button>
			</PageHeader>

			{models.length === 0 ? (
				<Card>
					<CardContent className="p-6 text-center text-sm text-muted-foreground">
						{t("virtualModels.emptyHint")}
					</CardContent>
				</Card>
			) : (
				<div className="space-y-6 pb-6">
					{models.map((vm) => (
						<VirtualModelSection
							key={vm.virtualModelId}
							virtualModel={vm}
							onEdit={setEditing}
							onDelete={setDeleting}
						/>
					))}
				</div>
			)}

			<VirtualModelEditDialog
				open={creating || editing !== null}
				onOpenChange={(open) => {
					if (!open) {
						setCreating(false);
						setEditing(null);
					}
				}}
				virtualModel={editing}
				providers={providers ?? []}
				providerModels={providerModels ?? []}
				mappedModelIds={mappedForEdit}
			/>

			<VirtualModelDeleteDialog
				open={deleting !== null}
				onOpenChange={(open) => {
					if (!open) setDeleting(null);
				}}
				virtualModel={deleting}
			/>
		</div>
	);
}
