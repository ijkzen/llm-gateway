import { EmptyState } from "@/components/empty-state";
import { ErrorState } from "@/components/error-state";
import { MidEllipsis } from "@/components/mid-ellipsis";
import { PageHeader } from "@/components/page-header";
import { PageHeaderSkeleton } from "@/components/page-header-skeleton";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { VirtualModelDeleteDialog } from "@/components/virtual-models/VirtualModelDeleteDialog";
import { VirtualModelEditDialog } from "@/components/virtual-models/VirtualModelEditDialog";
import { VirtualModelItemDetailDialog } from "@/components/virtual-models/VirtualModelItemDetailDialog";
import { VirtualModelSection } from "@/components/virtual-models/VirtualModelSection";
import { useProviderModels } from "@/hooks/use-provider-models";
import { useProviders } from "@/hooks/use-providers";
import {
	type VirtualModel,
	type VirtualModelItem,
	useVirtualModels,
} from "@/hooks/use-virtual-models";
import { VIRTUAL_MODELS_PAGE } from "@/lib/pages";
import { Plus, RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface SearchGroup {
	virtualModel: VirtualModel;
	items: VirtualModelItem[];
}

export default function VirtualModelsPage() {
	const { t } = useTranslation();
	const [creating, setCreating] = useState(false);
	const [editing, setEditing] = useState<VirtualModel | null>(null);
	const [deleting, setDeleting] = useState<VirtualModel | null>(null);
	/** 详情弹窗选中态：被点击成员与其所属虚拟模型。 */
	const [detail, setDetail] = useState<{
		virtualModel: VirtualModel;
		item: VirtualModelItem;
	} | null>(null);
	const [search, setSearch] = useState("");
	const [searchOpen, setSearchOpen] = useState(false);
	const searchRef = useRef<HTMLDivElement>(null);

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

	// 搜索结果：按成员 providerModelId 匹配，命中成员按所属虚拟模型分组。
	const searchGroups = useMemo<SearchGroup[]>(() => {
		const query = search.trim().toLowerCase();
		if (!query || !virtualModels) return [];

		return virtualModels.flatMap((vm) => {
			const matchingItems = vm.items.filter((item) =>
				item.providerModelId.toLowerCase().includes(query),
			);
			return matchingItems.length > 0 ? [{ virtualModel: vm, items: matchingItems }] : [];
		});
	}, [search, virtualModels]);

	useEffect(() => {
		const closeSearch = (event: PointerEvent) => {
			if (detail || searchRef.current?.contains(event.target as Node)) return;
			setSearchOpen(false);
		};
		document.addEventListener("pointerdown", closeSearch);
		return () => document.removeEventListener("pointerdown", closeSearch);
	}, [detail]);

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
						placeholder={t("virtualModels.search")}
						aria-label={t("virtualModels.search")}
						className="pl-9"
					/>
					{searchOpen && search.trim() && (
						<div className="absolute right-0 top-full z-20 mt-2 max-h-80 w-full overflow-y-auto rounded-xl border bg-popover p-2 shadow-lg">
							{searchGroups.length === 0 ? (
								<p className="px-3 py-2 text-sm text-muted-foreground">
									{t("virtualModels.searchNoResults")}
								</p>
							) : (
								<div className="space-y-3" data-testid="virtual-model-search-results">
									{searchGroups.map(({ virtualModel, items }) => (
										<div
											key={virtualModel.virtualModelId}
											data-testid={`virtual-model-search-group-${virtualModel.virtualModelId}`}
										>
											<p className="px-3 py-1 text-xs font-medium text-muted-foreground">
												{virtualModel.displayId}
											</p>
											{items.map((item) => (
												<button
													key={item.virtualModelItemId}
													type="button"
													onClick={() => setDetail({ virtualModel, item })}
													className="flex w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
												>
													<MidEllipsis text={item.providerModelId} className="min-w-0 font-mono" />
													<MidEllipsis
														text={item.providerName}
														className="shrink-0 text-xs text-muted-foreground"
													/>
												</button>
											))}
										</div>
									))}
								</div>
							)}
						</div>
					)}
				</div>
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
				<EmptyState
					title={t("virtualModels.emptyTitle")}
					description={t("virtualModels.emptyHint")}
				/>
			) : (
				<div className="space-y-6 pb-6">
					{models.map((vm) => (
						<VirtualModelSection
							key={vm.virtualModelId}
							virtualModel={vm}
							onEdit={setEditing}
							onDelete={setDeleting}
							onOpenItem={(virtualModel, item) => setDetail({ virtualModel, item })}
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

			<VirtualModelItemDetailDialog
				open={detail !== null}
				onOpenChange={(open) => {
					if (!open) setDetail(null);
				}}
				virtualModel={detail?.virtualModel ?? null}
				item={detail?.item ?? null}
			/>
		</div>
	);
}
