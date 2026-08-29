import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
	Dialog,
	DialogContent,
	DialogDescription,
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
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import {
	type CatalogCandidate,
	type MatchState,
	type ProviderModelPayload,
	type RefreshCandidate,
	useBatchCreateProviderModels,
	useCatalogSearch,
	useCreateProviderModel,
	useRefreshProviderModels,
} from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { RefreshCw, Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const CAPABILITY_KEYS = ["reasoning", "toolUse", "imageUnderstand", "videoUnderstand"] as const;

const manualFormSchema = z.object({
	providerModelId: z.string().min(1, "模型 ID 不能为空"),
	contextLength: z.coerce.number().int("必须为整数").positive("必须为正整数"),
	maxOutputTokens: z.coerce.number().int("必须为整数").positive("必须为正整数"),
	reasoning: z.boolean(),
	toolUse: z.boolean(),
	imageUnderstand: z.boolean(),
	videoUnderstand: z.boolean(),
});

type ManualFormValues = z.infer<typeof manualFormSchema>;

interface NumberEdits {
	contextLength: string;
	maxOutputTokens: string;
}

/** 解析正整数输入；不合法返回 null。 */
function parsePositiveInt(value: string): number | null {
	const trimmed = value.trim();
	if (!/^\d+$/.test(trimmed)) return null;
	const parsed = Number(trimmed);
	return parsed > 0 ? parsed : null;
}

function MatchStateLabel({ state }: { state: MatchState }) {
	if (state === "smart") {
		return (
			<span className="shrink-0 text-xs font-medium text-emerald-600 dark:text-emerald-400">
				已智能填充
			</span>
		);
	}
	if (state === "partial") {
		return (
			<span className="shrink-0 text-xs font-medium text-amber-600 dark:text-amber-400">
				信息不完整
			</span>
		);
	}
	return <span className="shrink-0 text-xs font-medium text-muted-foreground">需手动填写</span>;
}

interface AddProviderModelsDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	provider: Provider | null;
}

/** 添加供应商模型大弹窗：尝试刷新（候选多选导入）与手动添加并存。 */
export function AddProviderModelsDialog({
	open,
	onOpenChange,
	provider,
}: AddProviderModelsDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const providerId = provider?.id ?? 0;
	const refresh = useRefreshProviderModels(providerId);
	const batchCreate = useBatchCreateProviderModels(providerId);
	const createModel = useCreateProviderModel(providerId);

	const [candidates, setCandidates] = useState<RefreshCandidate[] | null>(null);
	const [numberEdits, setNumberEdits] = useState<Record<string, NumberEdits>>({});
	const [selected, setSelected] = useState<Set<string>>(new Set());
	// 手动添加的模型 ID 联想：防抖后的搜索关键词（空 = 不搜索）。
	const [modelSearchQuery, setModelSearchQuery] = useState("");
	const [modelSearchDebounced, setModelSearchDebounced] = useState("");
	// 已从目录选中的模型 ID：应用后隐藏下拉，直到用户重新输入。
	const [appliedModelId, setAppliedModelId] = useState<string | null>(null);
	const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

	const { data: catalogHits } = useCatalogSearch(modelSearchDebounced);

	// 弹窗打开时清空联想。
	useEffect(() => {
		if (!open) return;
		setModelSearchQuery("");
		setModelSearchDebounced("");
		setAppliedModelId(null);
		if (searchTimer.current) clearTimeout(searchTimer.current);
	}, [open]);

	// 输入防抖：停顿 300ms 后触发搜索；用户重新输入时恢复联想。
	const handleModelIdChange = (value: string) => {
		if (appliedModelId) setAppliedModelId(null);
		setModelSearchQuery(value);
		if (searchTimer.current) clearTimeout(searchTimer.current);
		searchTimer.current = setTimeout(() => setModelSearchDebounced(value), 300);
	};

	/** 点击候选：自动填充模型 ID 与全部字段（能力开关按目录预置）。 */
	const applyCatalogCandidate = (hit: CatalogCandidate) => {
		form.setValue("providerModelId", hit.id);
		form.setValue("contextLength", hit.contextLength ?? 0);
		form.setValue("maxOutputTokens", hit.maxOutputTokens ?? 0);
		form.setValue("reasoning", hit.reasoning);
		form.setValue("toolUse", hit.toolUse);
		form.setValue("imageUnderstand", hit.imageUnderstand);
		form.setValue("videoUnderstand", hit.videoUnderstand);
		setModelSearchQuery(hit.id);
		setModelSearchDebounced("");
		setAppliedModelId(hit.id);
	};

	const form = useForm<ManualFormValues>({
		resolver: zodResolver(manualFormSchema),
		defaultValues: {
			providerModelId: "",
			contextLength: 0,
			maxOutputTokens: 0,
			reasoning: false,
			toolUse: false,
			imageUnderstand: false,
			videoUnderstand: false,
		},
	});

	// 打开弹窗时重置全部状态（供应商只在弹窗关闭期间切换，无需依赖 provider.id）。
	useEffect(() => {
		if (!open) return;
		setCandidates(null);
		setNumberEdits({});
		setSelected(new Set());
		form.reset({
			providerModelId: "",
			contextLength: 0,
			maxOutputTokens: 0,
			reasoning: false,
			toolUse: false,
			imageUnderstand: false,
			videoUnderstand: false,
		});
	}, [open, form]);

	const handleRefresh = () => {
		if (!provider) return;
		refresh.mutate(undefined, {
			onSuccess: (list) => {
				setCandidates(list);
				const edits: Record<string, NumberEdits> = {};
				for (const item of list) {
					edits[item.providerModelId] = {
						contextLength: item.contextLength?.toString() ?? "",
						maxOutputTokens: item.maxOutputTokens?.toString() ?? "",
					};
				}
				setNumberEdits(edits);
				// 全部不预选，由用户自行勾选。
				setSelected(new Set());
			},
			onError: (error) => toastError("刷新失败", error),
		});
	};

	const editsOf = (candidate: RefreshCandidate): NumberEdits =>
		numberEdits[candidate.providerModelId] ?? { contextLength: "", maxOutputTokens: "" };

	const isSelectable = (candidate: RefreshCandidate): boolean =>
		parsePositiveInt(editsOf(candidate).contextLength) !== null &&
		parsePositiveInt(editsOf(candidate).maxOutputTokens) !== null;

	const toggleSelect = (candidate: RefreshCandidate, checked: boolean) => {
		setSelected((prev) => {
			const next = new Set(prev);
			if (checked) {
				next.add(candidate.providerModelId);
			} else {
				next.delete(candidate.providerModelId);
			}
			return next;
		});
	};

	const handleBatchAdd = () => {
		if (!provider) return;
		const list = (candidates ?? [])
			.filter((candidate) => selected.has(candidate.providerModelId))
			.map<ProviderModelPayload>((candidate) => ({
				providerModelId: candidate.providerModelId,
				contextLength: parsePositiveInt(editsOf(candidate).contextLength) ?? 0,
				maxOutputTokens: parsePositiveInt(editsOf(candidate).maxOutputTokens) ?? 0,
				reasoning: candidate.reasoning,
				toolUse: candidate.toolUse,
				imageUnderstand: candidate.imageUnderstand,
				videoUnderstand: candidate.videoUnderstand,
			}));
		if (list.some((item) => item.contextLength <= 0 || item.maxOutputTokens <= 0)) {
			toastError("添加失败", new Error("存在未填写完整的模型，请补齐数字字段"));
			return;
		}
		batchCreate.mutate(
			{ models: list },
			{
				onSuccess: (created) => {
					onOpenChange(false);
					toastSuccess(`已添加 ${created.length} 个模型`);
				},
				onError: (error) => toastError("添加失败", error),
			},
		);
	};

	const handleManualAdd = (values: ManualFormValues) => {
		if (!provider) return;
		createModel.mutate(
			{ ...values, providerModelId: values.providerModelId.trim() },
			{
				onSuccess: () => {
					toastSuccess("添加成功");
					form.reset({
						providerModelId: "",
						contextLength: 0,
						maxOutputTokens: 0,
						reasoning: false,
						toolUse: false,
						imageUnderstand: false,
						videoUnderstand: false,
					});
					setModelSearchQuery("");
					setModelSearchDebounced("");
					setAppliedModelId(null);
				},
				onError: (error) => toastError("添加失败", error),
			},
		);
	};

	if (!provider) return null;

	const selectedCount = selected.size;

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[760px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>添加供应商模型</DialogTitle>
					<DialogDescription>
						从 {provider.name} 拉取远端模型列表并智能填充；供应商未提供 Models 接口时可手动添加。
					</DialogDescription>
				</DialogHeader>

				<div className="flex items-center justify-between gap-4">
					<Button
						type="button"
						variant="outline"
						size="sm"
						onClick={handleRefresh}
						disabled={refresh.isPending}
					>
						<RefreshCw className={refresh.isPending ? "mr-2 size-4 animate-spin" : "mr-2 size-4"} />
						尝试刷新
					</Button>
					<span className="text-xs text-muted-foreground">拉取远端列表，按模型目录自动补全</span>
				</div>

				{candidates === null ? (
					<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
						尚未刷新，点击「尝试刷新」获取远端模型列表
					</div>
				) : candidates.length === 0 ? (
					<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
						远端未返回模型，或全部模型已导入；可使用下方手动添加
					</div>
				) : (
					<div className="space-y-3">
						<div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
							{candidates.map((candidate) => {
								const edits = editsOf(candidate);
								const selectable = isSelectable(candidate);
								return (
									<div key={candidate.providerModelId} className="rounded-lg border p-3">
										<div className="flex items-center gap-2.5">
											<Checkbox
												checked={selected.has(candidate.providerModelId)}
												disabled={!selectable}
												onCheckedChange={(checked) => toggleSelect(candidate, checked === true)}
												aria-label={`选择 ${candidate.providerModelId}`}
											/>
											<span
												className="min-w-0 flex-1 truncate font-mono text-sm"
												title={candidate.providerModelId}
											>
												{candidate.providerModelId}
											</span>
											<MatchStateLabel state={candidate.matchState} />
										</div>
										<div className="mt-2.5 grid grid-cols-2 gap-2">
											<div className="space-y-1">
												<Label className="text-xs text-muted-foreground">上下文长度</Label>
												<Input
													type="number"
													min={1}
													className="h-8"
													value={edits.contextLength}
													placeholder="必填"
													onChange={(e) =>
														setNumberEdits((prev) => ({
															...prev,
															[candidate.providerModelId]: {
																...edits,
																contextLength: e.target.value,
															},
														}))
													}
												/>
											</div>
											<div className="space-y-1">
												<Label className="text-xs text-muted-foreground">最大输出</Label>
												<Input
													type="number"
													min={1}
													className="h-8"
													value={edits.maxOutputTokens}
													placeholder="必填"
													onChange={(e) =>
														setNumberEdits((prev) => ({
															...prev,
															[candidate.providerModelId]: {
																...edits,
																maxOutputTokens: e.target.value,
															},
														}))
													}
												/>
											</div>
										</div>
									</div>
								);
							})}
						</div>
						<div className="flex items-center justify-between gap-4">
							<span className="text-xs text-muted-foreground">
								已选 {selectedCount} / 共 {candidates.length} 个
							</span>
							<Button
								type="button"
								size="sm"
								onClick={handleBatchAdd}
								disabled={selectedCount === 0 || batchCreate.isPending}
							>
								添加
							</Button>
						</div>
					</div>
				)}

				<Separator />

				{/* 手动添加：供应商未暴露 Models 接口或需要补录时使用。 */}
				<Form {...form}>
					<form
						id="provider-model-manual-form"
						onSubmit={form.handleSubmit(handleManualAdd)}
						className="space-y-4"
					>
						<h3 className="text-sm font-semibold">手动添加</h3>
						<FormField
							control={form.control}
							name="providerModelId"
							render={({ field }) => (
								<FormItem className="relative">
									<FormLabel required>模型 ID</FormLabel>
									<FormControl>
										<Input
											placeholder="如 gpt-4o"
											{...field}
											value={modelSearchQuery}
											onChange={(e) => {
												field.onChange(e);
												handleModelIdChange(e.target.value);
											}}
										/>
									</FormControl>
									{/* 关键词搜索联想下拉：点击候选自动填充全部字段；应用后隐藏 */}
									{(catalogHits?.length ?? 0) > 0 &&
										modelSearchQuery.trim().length > 0 &&
										!appliedModelId && (
											<div className="absolute inset-x-0 top-full z-20 mt-1 max-h-64 overflow-y-auto rounded-lg border border-input bg-popover p-1 shadow-lg backdrop-blur-xl">
												{catalogHits?.map((hit) => (
													<button
														key={hit.id}
														type="button"
														onClick={() => applyCatalogCandidate(hit)}
														className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-muted/60"
													>
														<Sparkles className="size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
														<span className="min-w-0">
															<span className="block truncate font-mono">{hit.id}</span>
															<span className="block truncate text-xs text-muted-foreground">
																{hit.name}
																{hit.family ? ` · ${hit.family}` : ""}
															</span>
														</span>
													</button>
												))}
											</div>
										)}
									<FormMessage />
								</FormItem>
							)}
						/>
						<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
							<FormField
								control={form.control}
								name="contextLength"
								render={({ field }) => (
									<FormItem>
										<FormLabel required>上下文长度</FormLabel>
										<FormControl>
											<Input type="number" min={1} placeholder="如 128000" {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<FormField
								control={form.control}
								name="maxOutputTokens"
								render={({ field }) => (
									<FormItem>
										<FormLabel required>最大输出</FormLabel>
										<FormControl>
											<Input type="number" min={1} placeholder="如 4096" {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
						</div>
						<div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
							{CAPABILITY_KEYS.map((key) => (
								<FormField
									key={key}
									control={form.control}
									name={key}
									render={({ field }) => (
										<FormItem className="flex items-center justify-between rounded-lg border p-3">
											<FormLabel>
												{
													{
														reasoning: "推理",
														toolUse: "工具调用",
														imageUnderstand: "图像理解",
														videoUnderstand: "视频理解",
													}[key]
												}
											</FormLabel>
											<FormControl>
												<Switch checked={field.value} onCheckedChange={field.onChange} />
											</FormControl>
										</FormItem>
									)}
								/>
							))}
						</div>
						<div className="flex justify-end">
							<Button type="submit" size="sm" disabled={createModel.isPending}>
								手动添加
							</Button>
						</div>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
