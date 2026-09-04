import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import {
	Form,
	FormControl,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { ItemCapabilityIcons } from "@/components/virtual-models/ItemCapabilityIcons";
import {
	type DraftItem,
	type DraftMember,
	compareDraftMembers,
} from "@/components/virtual-models/draft-members";
import type { ProviderModel } from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import {
	type VirtualModel,
	type VirtualModelItemPayload,
	useCreateVirtualModel,
	useUpdateVirtualModel,
} from "@/hooks/use-virtual-models";
import { FALLBACK_STRATEGIES, LOAD_BALANCING_STRATEGIES } from "@/lib/constants";
import { cn, formatContextLength } from "@/lib/utils";
import { zodResolver } from "@hookform/resolvers/zod";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

function makeFormSchema(t: (key: string) => string) {
	return z.object({
		displayId: z.string().trim().min(1, t("virtualModels.displayIdRequired")),
		enable: z.boolean(),
		loadBalancingStrategy: z.number(),
		fallbackStrategy: z.number(),
	});
}

type FormValues = z.infer<ReturnType<typeof makeFormSchema>>;

interface VirtualModelEditDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	/** 编辑目标；null 表示创建模式。 */
	virtualModel: VirtualModel | null;
	providers: Provider[];
	providerModels: ProviderModel[];
	/** 已被其他虚拟模型映射的 modelId 集合（页面计算，已排除当前编辑目标自身的成员）。 */
	mappedModelIds: Set<number>;
}

/** 供应商分组：成员（组内已按可用性 + LB 顺序排序）与可添加候选。 */
interface ProviderGroup {
	provider: Provider;
	rows: DraftMember[];
	candidates: ProviderModel[];
}

/** 分组头键盘折叠/展开：← 折叠、→ 展开；Enter/Space 由按钮原生切换。 */
function onHeaderKeyDown(
	event: React.KeyboardEvent,
	expanded: boolean,
	setExpanded: (next: boolean) => void,
) {
	if (event.key === "ArrowLeft" && expanded) setExpanded(false);
	if (event.key === "ArrowRight" && !expanded) setExpanded(true);
}

/**
 * 创建/编辑虚拟模型弹窗：标题栏与底部操作栏固定，中间内容区滚动；
 * 成员模型分「已使用 / 未使用」两个 Tab 按供应商分组展示——组内成员先按
 * 启用状态分组（可用在前），组内再按 LB 顺序（virtualModelItemId 升序）排序；
 * 分组头支持鼠标与方向键折叠/展开（与供应商启用状态无关）。
 */
export function VirtualModelEditDialog({
	open,
	onOpenChange,
	virtualModel,
	providers,
	providerModels,
	mappedModelIds,
}: VirtualModelEditDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const createModel = useCreateVirtualModel();
	const updateModel = useUpdateVirtualModel();
	const formSchema = useMemo(() => makeFormSchema(t), [t]);
	const [draftItems, setDraftItems] = useState<DraftItem[]>([]);
	const [activeTab, setActiveTab] = useState<"used" | "unused">("used");
	/** 已折叠（收起整组）的供应商 id 集合。 */
	const [collapsedGroups, setCollapsedGroups] = useState<Set<number>>(new Set());
	/** 组内「展开候选区」的供应商 id 集合（已使用 Tab 内继续添加）。 */
	const [openAddGroups, setOpenAddGroups] = useState<Set<number>>(new Set());

	const form = useForm<FormValues>({
		resolver: zodResolver(formSchema),
		defaultValues: {
			displayId: "",
			enable: true,
			loadBalancingStrategy: 0,
			fallbackStrategy: 0,
		},
	});

	// 打开弹窗时以目标虚拟模型重置全部暂存状态。
	useEffect(() => {
		if (!open) return;
		form.reset({
			displayId: virtualModel?.displayId ?? "",
			enable: virtualModel?.enable ?? true,
			loadBalancingStrategy: virtualModel?.loadBalancingStrategy ?? 0,
			fallbackStrategy: virtualModel?.fallbackStrategy ?? 0,
		});
		setDraftItems(
			(virtualModel?.items ?? []).map((item) => ({
				virtualModelItemId: item.virtualModelItemId,
				modelId: item.modelId,
				enable: item.enable,
			})),
		);
		// 创建模式没有既有成员，默认落在「未使用」Tab；编辑模式默认「已使用」。
		setActiveTab((virtualModel?.items.length ?? 0) > 0 ? "used" : "unused");
		setCollapsedGroups(new Set());
		setOpenAddGroups(new Set());
	}, [open, virtualModel, form]);

	const modelById = new Map(providerModels.map((model) => [model.modelId, model]));
	const providerById = new Map(providers.map((provider) => [provider.id, provider]));
	const providerEnabledOf = (model: ProviderModel) =>
		providerById.get(model.providerId)?.enable ?? false;

	const addDraftItem = (modelId: number) => {
		setDraftItems((prev) => [...prev, { virtualModelItemId: null, modelId, enable: true }]);
	};

	const removeDraftItem = (modelId: number) => {
		setDraftItems((prev) => prev.filter((item) => item.modelId !== modelId));
	};

	const toggleDraftEnable = (modelId: number) => {
		setDraftItems((prev) =>
			prev.map((item) => (item.modelId === modelId ? { ...item, enable: !item.enable } : item)),
		);
	};

	const setGroupCollapsed = (providerId: number, collapsed: boolean) => {
		setCollapsedGroups((prev) => {
			const next = new Set(prev);
			if (collapsed) {
				next.add(providerId);
			} else {
				next.delete(providerId);
			}
			return next;
		});
	};

	const toggleGroupCollapsed = (providerId: number) => {
		setCollapsedGroups((prev) => {
			const next = new Set(prev);
			if (next.has(providerId)) {
				next.delete(providerId);
			} else {
				next.add(providerId);
			}
			return next;
		});
	};

	const toggleAddGroup = (providerId: number) => {
		setOpenAddGroups((prev) => {
			const next = new Set(prev);
			if (next.has(providerId)) {
				next.delete(providerId);
			} else {
				next.add(providerId);
			}
			return next;
		});
	};

	// 候选 = 该供应商名下、未被其他虚拟模型占用且尚未加入暂存的模型。
	const candidatesOf = (providerId: number) =>
		providerModels.filter(
			(model) =>
				model.providerId === providerId &&
				!mappedModelIds.has(model.modelId) &&
				!draftItems.some((draft) => draft.modelId === model.modelId),
		);

	/** 供应商分组：成员行（join 供应商模型并组内排序）+ 候选；组存在性由调用方过滤。 */
	const groupOf = (provider: Provider): ProviderGroup => {
		const rows = draftItems
			.flatMap((draft) => {
				const model = modelById.get(draft.modelId);
				return model !== undefined && model.providerId === provider.id ? [{ draft, model }] : [];
			})
			.sort((a, b) => compareDraftMembers(a, b, providerEnabledOf));
		return { provider, rows, candidates: candidatesOf(provider.id) };
	};

	// 已使用：有成员的供应商组；未使用：无成员但有可添加候选的供应商组。均按 providers 顺序。
	const usedGroups = providers.map(groupOf).filter((group) => group.rows.length > 0);
	const unusedGroups = providers
		.map(groupOf)
		.filter((group) => group.rows.length === 0 && group.candidates.length > 0);

	const onSubmit = (values: FormValues) => {
		if (draftItems.length === 0) return;
		const items: VirtualModelItemPayload[] = draftItems.map(({ modelId, enable }) => ({
			modelId,
			enable,
		}));
		const body = {
			displayId: values.displayId.trim(),
			enable: values.enable,
			loadBalancingStrategy: values.loadBalancingStrategy,
			fallbackStrategy: values.fallbackStrategy,
			items,
		};
		const options = {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess(virtualModel ? t("common.updateSuccess") : t("common.createSuccess"));
			},
			onError: (error: Error) =>
				toastError(virtualModel ? t("common.updateFailed") : t("common.createFailed"), error),
		};
		if (virtualModel) {
			updateModel.mutate({ id: virtualModel.virtualModelId, ...body }, options);
		} else {
			createModel.mutate(body, options);
		}
	};

	const renderGroup = (group: ProviderGroup) => {
		const expanded = !collapsedGroups.has(group.provider.id);
		const addOpen = openAddGroups.has(group.provider.id);
		const providerDisabled = !group.provider.enable;
		return (
			<div key={group.provider.id} className="space-y-2">
				<button
					type="button"
					aria-expanded={expanded}
					onClick={() => toggleGroupCollapsed(group.provider.id)}
					onKeyDown={(event) =>
						onHeaderKeyDown(event, expanded, (nextExpanded) =>
							setGroupCollapsed(group.provider.id, !nextExpanded),
						)
					}
					className="flex w-full items-center justify-between gap-2 rounded-lg px-1 py-1 text-left transition-colors hover:bg-muted/60"
				>
					<span className="flex min-w-0 items-center gap-2">
						<span className="truncate text-sm font-medium">{group.provider.name}</span>
						{providerDisabled && (
							<span className="shrink-0 text-xs text-warning">
								{t("virtualModels.providerDisabled")}
							</span>
						)}
					</span>
					<ChevronRight
						aria-hidden="true"
						className={cn(
							"size-4 shrink-0 text-muted-foreground transition-transform",
							expanded && "rotate-90",
						)}
					/>
				</button>

				{expanded && (
					<div className="space-y-2 pl-1">
						{group.rows.length > 0 && (
							<div className="space-y-2">
								{group.rows.map(({ draft, model }) => (
									<div
										key={draft.modelId}
										className={cn(
											"flex items-center gap-3 rounded-lg border px-3 py-2",
											(draft.enable === false || providerDisabled) && "opacity-60",
										)}
									>
										<div className="min-w-0 flex-1">
											<p className="truncate font-mono text-sm" title={model.providerModelId}>
												{model.providerModelId}
											</p>
											<p className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
												<span className="shrink-0">{formatContextLength(model.contextLength)}</span>
												{providerDisabled && (
													<span className="shrink-0 text-warning">
														{t("virtualModels.disabledWithProvider")}
													</span>
												)}
												{draft.enable === false && (
													<span className="shrink-0">{t("virtualModels.disabledMark")}</span>
												)}
											</p>
										</div>
										<ItemCapabilityIcons item={model} className="shrink-0" />
										<Switch
											checked={draft.enable}
											disabled={updateModel.isPending || createModel.isPending}
											aria-label={`${t("virtualModels.toggleMember")} ${model.providerModelId}`}
											onCheckedChange={() => toggleDraftEnable(draft.modelId)}
										/>
										<Button
											type="button"
											variant="ghost"
											size="icon"
											className="size-8 shrink-0 text-destructive hover:text-destructive"
											aria-label={`${t("virtualModels.removeMember")} ${model.providerModelId}`}
											onClick={() => removeDraftItem(draft.modelId)}
										>
											<Trash2 className="size-4" />
										</Button>
									</div>
								))}
							</div>
						)}

						{group.rows.length > 0 && group.candidates.length > 0 && (
							<Button
								type="button"
								variant="outline"
								size="sm"
								aria-label={t("virtualModels.addInProvider", { provider: group.provider.name })}
								onClick={() => toggleAddGroup(group.provider.id)}
							>
								<Plus className="size-4" />
								{t("virtualModels.addMore")}
							</Button>
						)}

						{(addOpen || group.rows.length === 0) && group.candidates.length > 0 && (
							<div className="space-y-2 rounded-lg border border-dashed p-3">
								{group.candidates.map((model) => (
									<div
										key={model.modelId}
										className="flex items-center gap-2.5 rounded-lg border p-2.5"
									>
										<Button
											type="button"
											variant="outline"
											size="icon"
											className="size-7 shrink-0"
											aria-label={`${t("virtualModels.addCandidate")} ${model.providerModelId}`}
											onClick={() => addDraftItem(model.modelId)}
										>
											<Plus className="size-4" />
										</Button>
										<span
											className="min-w-0 flex-1 truncate font-mono text-sm"
											title={model.providerModelId}
										>
											{model.providerModelId}
										</span>
										<span className="shrink-0 text-xs text-muted-foreground">
											{formatContextLength(model.contextLength)}
										</span>
									</div>
								))}
							</div>
						)}
					</div>
				)}
			</div>
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="flex h-[min(720px,85vh)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[680px]">
				<DialogHeader className="shrink-0 space-y-3 px-6 pb-4 pt-6">
					<DialogTitle>
						{virtualModel ? t("virtualModels.editTitle") : t("virtualModels.createTitle")}
					</DialogTitle>
					<DialogDescription>{t("virtualModels.editDesc")}</DialogDescription>
				</DialogHeader>

				<div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
					<Form {...form}>
						<form
							id="virtual-model-form"
							onSubmit={form.handleSubmit(onSubmit)}
							className="space-y-4"
						>
							<FormField
								control={form.control}
								name="displayId"
								render={({ field }) => (
									<FormItem>
										<FormLabel required>{t("common.modelId")}</FormLabel>
										<FormControl>
											<Input placeholder={t("providerModels.modelIdPlaceholder")} {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
								<FormField
									control={form.control}
									name="loadBalancingStrategy"
									render={({ field }) => (
										<FormItem>
											<FormLabel>{t("virtualModels.loadBalancing")}</FormLabel>
											<Select
												onValueChange={(v) => field.onChange(Number(v))}
												value={String(field.value)}
											>
												<FormControl>
													<SelectTrigger>
														<SelectValue placeholder={t("virtualModels.selectStrategy")} />
													</SelectTrigger>
												</FormControl>
												<SelectContent>
													{LOAD_BALANCING_STRATEGIES.map((s) => (
														<SelectItem key={s.value} value={String(s.value)}>
															{t(s.labelKey)}
														</SelectItem>
													))}
												</SelectContent>
											</Select>
											<FormMessage />
										</FormItem>
									)}
								/>
								<FormField
									control={form.control}
									name="fallbackStrategy"
									render={({ field }) => (
										<FormItem>
											<FormLabel>{t("virtualModels.fallback")}</FormLabel>
											<Select
												onValueChange={(v) => field.onChange(Number(v))}
												value={String(field.value)}
											>
												<FormControl>
													<SelectTrigger>
														<SelectValue placeholder={t("virtualModels.selectStrategy")} />
													</SelectTrigger>
												</FormControl>
												<SelectContent>
													{FALLBACK_STRATEGIES.map((s) => (
														<SelectItem key={s.value} value={String(s.value)}>
															{t(s.labelKey)}
														</SelectItem>
													))}
												</SelectContent>
											</Select>
											<FormMessage />
										</FormItem>
									)}
								/>
							</div>
							<FormField
								control={form.control}
								name="enable"
								render={({ field }) => (
									<FormItem className="flex items-center justify-between rounded-lg border p-3">
										<div className="space-y-0.5">
											<FormLabel>{t("virtualModels.enable")}</FormLabel>
											<p className="text-xs text-muted-foreground">
												{t("virtualModels.disableHint")}
											</p>
										</div>
										<FormControl>
											<Switch checked={field.value} onCheckedChange={field.onChange} />
										</FormControl>
									</FormItem>
								)}
							/>
							<div role="tablist" aria-label={t("virtualModels.members")} className="flex gap-1">
								<Button
									type="button"
									role="tab"
									variant="ghost"
									size="sm"
									aria-selected={activeTab === "used"}
									onClick={() => setActiveTab("used")}
								>
									{t("virtualModels.usedTab")}
								</Button>
								<Button
									type="button"
									role="tab"
									variant="ghost"
									size="sm"
									aria-selected={activeTab === "unused"}
									onClick={() => setActiveTab("unused")}
								>
									{t("virtualModels.unusedTab")}
								</Button>
							</div>
						</form>
					</Form>

					<Separator className="my-5" />

					<div className="space-y-4">
						{activeTab === "used" ? (
							usedGroups.length === 0 ? (
								<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
									{t("virtualModels.usedEmptyHint")}
								</div>
							) : (
								usedGroups.map(renderGroup)
							)
						) : unusedGroups.length === 0 ? (
							<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
								{t("virtualModels.unusedEmptyHint")}
							</div>
						) : (
							unusedGroups.map(renderGroup)
						)}
					</div>
				</div>

				<DialogFooter className="shrink-0 gap-2 border-t px-6 py-4">
					<span className="mr-auto text-xs text-muted-foreground">
						{draftItems.length === 0
							? t("virtualModels.keepAtLeastOne")
							: t("virtualModels.selectedMembers", { count: draftItems.length })}
					</span>
					<Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
						{t("common.cancel")}
					</Button>
					<Button
						type="submit"
						size="sm"
						form="virtual-model-form"
						disabled={draftItems.length === 0 || createModel.isPending || updateModel.isPending}
					>
						{virtualModel ? t("common.save") : t("common.create")}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
