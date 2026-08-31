import { ProtocolIcon } from "@/components/providers/ProtocolIcon";
import { Card, CardContent } from "@/components/ui/card";
import { type Provider, providerKeys, useReorderProviders } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { cn } from "@/lib/utils";
import {
	DndContext,
	type DragEndEvent,
	KeyboardSensor,
	PointerSensor,
	closestCenter,
	useSensor,
	useSensors,
} from "@dnd-kit/core";
import {
	SortableContext,
	arrayMove,
	sortableKeyboardCoordinates,
	useSortable,
	verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useQueryClient } from "@tanstack/react-query";
import { GripVertical } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ProviderListProps {
	providers: Provider[] | undefined;
	selectedId: number | null;
	onSelect: (provider: Provider) => void;
}

interface SortableProviderRowProps {
	provider: Provider;
	selected: boolean;
	onSelect: () => void;
}

function SortableProviderRow({ provider, selected, onSelect }: SortableProviderRowProps) {
	const { t } = useTranslation();
	const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
		id: provider.id,
	});
	const style = {
		transform: CSS.Transform.toString(transform),
		transition,
	};

	return (
		<li ref={setNodeRef} style={style} className={cn(isDragging && "relative z-10 opacity-80")}>
			<button
				type="button"
				onClick={onSelect}
				title={`${provider.name}（${provider.enable ? t("common.enabled") : t("common.disabled")}）`}
				className={cn(
					"flex w-full items-center gap-3 rounded-lg px-4 py-3 text-left transition-colors",
					selected
						? "bg-foreground text-background dark:bg-primary dark:text-primary-foreground"
						: "hover:bg-slate-100/60 dark:hover:bg-white/5",
				)}
			>
				<GripVertical
					{...attributes}
					{...listeners}
					className={cn(
						"size-4 shrink-0 cursor-grab touch-none text-muted-foreground active:cursor-grabbing",
						selected && "text-background/70 dark:text-primary-foreground/70",
					)}
				/>
				<ProtocolIcon
					protocolType={provider.protocolType}
					className={
						selected ? "text-background dark:text-primary-foreground" : "text-muted-foreground"
					}
				/>
				<span className="min-w-0 flex-1 truncate font-medium">{provider.name}</span>
				<span
					className={cn(
						"size-2 shrink-0 rounded-full",
						provider.enable ? "bg-emerald-500" : "bg-red-500",
					)}
					aria-label={provider.enable ? t("common.enabled") : t("common.disabled")}
				/>
			</button>
		</li>
	);
}

export function ProviderList({ providers, selectedId, onSelect }: ProviderListProps) {
	const { t } = useTranslation();
	const queryClient = useQueryClient();
	const { toastSuccess, toastError } = useToastActions();
	const { mutate: reorder } = useReorderProviders();
	const sensors = useSensors(
		useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
		useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
	);

	if (!providers || providers.length === 0) {
		return (
			<Card>
				<CardContent className="p-8 text-center text-muted-foreground">
					{t("providers.noProvidersHint")}
				</CardContent>
			</Card>
		);
	}

	const items = providers;

	function handleDragEnd(event: DragEndEvent) {
		const { active, over } = event;
		if (!over) return;

		// 拖拽后的目标 id 顺序；拖到原位置/找不到时返回 null，不触发重排。
		const nextIds = computeReorderIds(items, active.id as number, over.id as number);
		if (nextIds === null) return;

		// 先本地乐观更新（拖完立即见效），接口返回后再以服务端数据为准。
		const next = nextIds
			.map((id) => items.find((p) => p.id === id))
			.filter((p): p is Provider => p !== undefined);
		queryClient.setQueryData(providerKeys.all, next);

		reorder(nextIds, {
			onSuccess: () => toastSuccess(t("providers.reorderSuccess")),
			onError: (error) => toastError(t("providers.reorderFailedText"), error),
		});
	}

	return (
		<Card className="p-0">
			<CardContent className="p-0">
				<DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
					<SortableContext
						items={providers.map((p) => p.id)}
						strategy={verticalListSortingStrategy}
					>
						<ul className="space-y-1 p-1.5">
							{providers.map((provider) => (
								<SortableProviderRow
									key={provider.id}
									provider={provider}
									selected={selectedId === provider.id}
									onSelect={() => onSelect(provider)}
								/>
							))}
						</ul>
					</SortableContext>
				</DndContext>
			</CardContent>
		</Card>
	);
}

/**
 * 计算拖拽后的目标 id 顺序：把 activeId 移到 overId 的位置。
 * 拖到原位置或找不到任一 id 时返回 null（不触发重排）。
 */
export function computeReorderIds<T extends { id: number }>(
	items: T[],
	activeId: number,
	overId: number,
): number[] | null {
	if (activeId === overId) return null;
	const oldIndex = items.findIndex((p) => p.id === activeId);
	const newIndex = items.findIndex((p) => p.id === overId);
	if (oldIndex === -1 || newIndex === -1) return null;
	return arrayMove(
		items.map((p) => p.id),
		oldIndex,
		newIndex,
	);
}
