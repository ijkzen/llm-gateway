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
import { ChevronDown, ChevronUp, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const formSchema = z.object({
	displayId: z.string().trim().min(1, "模型 ID 不能为空"),
	enable: z.boolean(),
	loadBalancingStrategy: z.number(),
	fallbackStrategy: z.number(),
});

type FormValues = z.infer<typeof formSchema>;

/** 暂存的成员条目：加入弹窗时的启停状态随行保存。 */
interface DraftItem {
	modelId: number;
	enable: boolean;
}

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

/**
 * 创建/编辑虚拟模型弹窗：顶部基本信息（模型 ID、启停、策略），
 * 下方按供应商分组管理成员——添加、删除、启停均在弹窗内暂存，点「保存」一次性生效。
 */
export function VirtualModelEditDialog({
	open,
	onOpenChange,
	virtualModel,
	providers,
	providerModels,
	mappedModelIds,
}: VirtualModelEditDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const createModel = useCreateVirtualModel();
	const updateModel = useUpdateVirtualModel();
	const [draftItems, setDraftItems] = useState<DraftItem[]>([]);
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
			(virtualModel?.items ?? []).map((item) => ({ modelId: item.modelId, enable: item.enable })),
		);
		setOpenAddGroups(new Set());
	}, [open, virtualModel, form]);

	const modelById = new Map(providerModels.map((model) => [model.modelId, model]));

	const addDraftItem = (modelId: number) => {
		setDraftItems((prev) => [...prev, { modelId, enable: true }]);
	};

	const removeDraftItem = (modelId: number) => {
		setDraftItems((prev) => prev.filter((item) => item.modelId !== modelId));
	};

	const toggleDraftEnable = (modelId: number) => {
		setDraftItems((prev) =>
			prev.map((item) => (item.modelId === modelId ? { ...item, enable: !item.enable } : item)),
		);
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

	const onSubmit = (values: FormValues) => {
		if (draftItems.length === 0) return;
		const items: VirtualModelItemPayload[] = draftItems.map((item) => ({
			modelId: item.modelId,
			enable: item.enable,
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
				toastSuccess(virtualModel ? "保存成功" : "创建成功");
			},
			onError: (error: Error) => toastError(virtualModel ? "保存失败" : "创建失败", error),
		};
		if (virtualModel) {
			updateModel.mutate({ id: virtualModel.virtualModelId, ...body }, options);
		} else {
			createModel.mutate(body, options);
		}
	};

	// 候选 = 该供应商名下、未被其他虚拟模型占用且尚未加入暂存的模型。
	const candidatesOf = (providerId: number) =>
		providerModels.filter(
			(model) =>
				model.providerId === providerId &&
				!mappedModelIds.has(model.modelId) &&
				!draftItems.some((draft) => draft.modelId === model.modelId),
		);

	// 按供应商分组渲染暂存成员；组内有成员、有可添加候选或已展开添加区时才显示。
	const draftGroups = providers
		.map((provider) => ({
			provider,
			rows: draftItems.flatMap((draft) => {
				const model = modelById.get(draft.modelId);
				return model !== undefined && model.providerId === provider.id ? [{ draft, model }] : [];
			}),
		}))
		.filter(
			(group) =>
				group.rows.length > 0 ||
				candidatesOf(group.provider.id).length > 0 ||
				openAddGroups.has(group.provider.id),
		);

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[680px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>{virtualModel ? "编辑虚拟模型" : "添加虚拟模型"}</DialogTitle>
					<DialogDescription>
						按供应商管理映射的模型：可添加、删除与启停成员；修改先暂存，点「保存」一次性生效。
					</DialogDescription>
				</DialogHeader>

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
									<FormLabel required>模型 ID</FormLabel>
									<FormControl>
										<Input placeholder="如 gpt-4o" {...field} />
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
										<FormLabel>负载均衡策略</FormLabel>
										<Select
											onValueChange={(v) => field.onChange(Number(v))}
											value={String(field.value)}
										>
											<FormControl>
												<SelectTrigger>
													<SelectValue placeholder="选择策略" />
												</SelectTrigger>
											</FormControl>
											<SelectContent>
												{LOAD_BALANCING_STRATEGIES.map((s) => (
													<SelectItem key={s.value} value={String(s.value)}>
														{s.label}
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
										<FormLabel>降级策略</FormLabel>
										<Select
											onValueChange={(v) => field.onChange(Number(v))}
											value={String(field.value)}
										>
											<FormControl>
												<SelectTrigger>
													<SelectValue placeholder="选择策略" />
												</SelectTrigger>
											</FormControl>
											<SelectContent>
												{FALLBACK_STRATEGIES.map((s) => (
													<SelectItem key={s.value} value={String(s.value)}>
														{s.label}
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
										<FormLabel>启用</FormLabel>
										<p className="text-xs text-muted-foreground">
											禁用后该虚拟模型不会出现在 /v1/models 中
										</p>
									</div>
									<FormControl>
										<Switch checked={field.value} onCheckedChange={field.onChange} />
									</FormControl>
								</FormItem>
							)}
						/>

						<Separator />

						<div className="space-y-4">
							<h3 className="text-sm font-semibold">成员模型</h3>
							{draftGroups.length === 0 ? (
								<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
									暂无可管理的成员。已被其他虚拟模型占用的模型不会出现在候选中。
								</div>
							) : (
								draftGroups.map((group) => {
									const candidates = candidatesOf(group.provider.id);
									const addOpen = openAddGroups.has(group.provider.id);
									return (
										<div key={group.provider.id} className="space-y-2">
											<div className="flex items-center justify-between gap-4">
												<div className="flex min-w-0 items-center gap-2">
													<h4 className="text-sm font-medium">{group.provider.name}</h4>
													{!group.provider.enable && (
														<span className="shrink-0 text-xs text-amber-600 dark:text-amber-400">
															供应商已禁用
														</span>
													)}
												</div>
												<Button
													type="button"
													variant="outline"
													size="icon"
													className="size-8"
													onClick={() => toggleAddGroup(group.provider.id)}
													disabled={!addOpen && candidates.length === 0}
													aria-label={`在 ${group.provider.name} 中添加模型`}
												>
													{addOpen ? (
														<ChevronUp className="size-4" />
													) : (
														<ChevronDown className="size-4" />
													)}
												</Button>
											</div>

											{group.rows.length > 0 && (
												<div className="space-y-2">
													{group.rows.map(({ draft, model }) => {
														const providerDisabled = !group.provider.enable;
														return (
															<div
																key={draft.modelId}
																className={cn(
																	"flex items-center gap-3 rounded-lg border px-3 py-2",
																	(draft.enable === false || providerDisabled) && "opacity-60",
																)}
															>
																<div className="min-w-0 flex-1">
																	<p
																		className="truncate font-mono text-sm"
																		title={model.providerModelId}
																	>
																		{model.providerModelId}
																	</p>
																	<p className="mt-0.5 flex items-center gap-1.5 text-xs text-muted-foreground">
																		<span className="shrink-0">
																			{formatContextLength(model.contextLength)} ctx
																		</span>
																		{providerDisabled && (
																			<span className="shrink-0 text-amber-600 dark:text-amber-400">
																				· 随供应商禁用
																			</span>
																		)}
																		{draft.enable === false && (
																			<span className="shrink-0">· 已停用</span>
																		)}
																	</p>
																</div>
																<ItemCapabilityIcons item={model} className="shrink-0" />
																<Switch
																	checked={draft.enable}
																	disabled={updateModel.isPending || createModel.isPending}
																	aria-label={`启停 ${model.providerModelId}`}
																	onCheckedChange={() => toggleDraftEnable(draft.modelId)}
																/>
																<Button
																	type="button"
																	variant="ghost"
																	size="icon"
																	className="size-8 shrink-0 text-destructive hover:text-destructive"
																	aria-label={`移除 ${model.providerModelId}`}
																	onClick={() => removeDraftItem(draft.modelId)}
																>
																	<Trash2 className="size-4" />
																</Button>
															</div>
														);
													})}
												</div>
											)}

											{addOpen && (
												<div className="space-y-2 rounded-lg border border-dashed p-3">
													{candidates.length === 0 ? (
														<p className="py-2 text-center text-xs text-muted-foreground">
															没有可添加的模型（已被占用或已全部加入）
														</p>
													) : (
														candidates.map((model) => (
															<div
																key={model.modelId}
																className="flex items-center gap-2.5 rounded-lg border p-2.5"
															>
																<Button
																	type="button"
																	variant="outline"
																	size="icon"
																	className="size-7 shrink-0"
																	aria-label={`添加 ${model.providerModelId}`}
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
																	{formatContextLength(model.contextLength)} ctx
																</span>
															</div>
														))
													)}
												</div>
											)}
										</div>
									);
								})
							)}
						</div>
					</form>
				</Form>

				<DialogFooter className="gap-2 pt-2">
					<span className="mr-auto text-xs text-muted-foreground">
						{draftItems.length === 0
							? "至少保留一个成员模型"
							: `已选 ${draftItems.length} 个成员模型`}
					</span>
					<Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
						取消
					</Button>
					<Button
						type="submit"
						size="sm"
						form="virtual-model-form"
						disabled={draftItems.length === 0 || createModel.isPending || updateModel.isPending}
					>
						{virtualModel ? "保存" : "创建"}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
