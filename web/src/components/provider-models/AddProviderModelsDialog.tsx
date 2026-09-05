import { ConfirmDialog } from "@/components/confirm-dialog";
import { badgeVariants } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
	type CatalogCandidate,
	type CatalogSuggestion,
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
import { cn } from "@/lib/utils";
import { zodResolver } from "@hookform/resolvers/zod";
import { RefreshCw, Search, Sparkles } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";
const CAPABILITY_KEYS = ["reasoning", "toolUse", "imageUnderstand", "videoUnderstand"] as const;

const CAPABILITY_LABEL_KEYS: Record<(typeof CAPABILITY_KEYS)[number], string> = {
	reasoning: "providerModels.reasoning",
	toolUse: "providerModels.toolUse",
	imageUnderstand: "providerModels.imageUnderstand",
	videoUnderstand: "providerModels.videoUnderstand",
};

function makeManualFormSchema(t: (key: string) => string) {
	return z.object({
		providerModelId: z.string().min(1, t("providerModels.modelIdRequired")),
		contextLength: z.coerce
			.number()
			.int(t("providerModels.mustBeInt"))
			.positive(t("providerModels.mustBePositive")),
		maxOutputTokens: z.coerce
			.number()
			.int(t("providerModels.mustBeInt"))
			.positive(t("providerModels.mustBePositive")),
		reasoning: z.boolean(),
		toolUse: z.boolean(),
		imageUnderstand: z.boolean(),
		videoUnderstand: z.boolean(),
	});
}

type ManualFormValues = z.infer<ReturnType<typeof makeManualFormSchema>>;

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
	const { t } = useTranslation();
	if (state === "smart") {
		return (
			<span className="shrink-0 text-xs font-medium text-success">
				{t("providerModels.smartFilled")}
			</span>
		);
	}
	if (state === "partial") {
		return (
			<span className="shrink-0 text-xs font-medium text-warning">
				{t("providerModels.partialInfo")}
			</span>
		);
	}
	if (state === "pending") {
		return (
			<span className="shrink-0 text-xs font-medium text-primary">
				{t("providerModels.pendingConfirm")}
			</span>
		);
	}
	return (
		<span className="shrink-0 text-xs font-medium text-muted-foreground">
			{t("providerModels.needManual")}
		</span>
	);
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
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const providerId = provider?.id ?? 0;
	const refresh = useRefreshProviderModels(providerId);
	const batchCreate = useBatchCreateProviderModels(providerId);
	const createModel = useCreateProviderModel(providerId);
	const manualFormSchema = useMemo(() => makeManualFormSchema(t), [t]);

	const [candidates, setCandidates] = useState<RefreshCandidate[] | null>(null);
	const [numberEdits, setNumberEdits] = useState<Record<string, NumberEdits>>({});
	const [selected, setSelected] = useState<Set<string>>(new Set());
	const [activeTab, setActiveTab] = useState<"auto" | "manual">("auto");
	const [catalogOpen, setCatalogOpen] = useState(false);
	const catalogRef = useRef<HTMLDivElement>(null);
	// 手动添加的模型 ID 联想：防抖后的搜索关键词（空 = 不搜索）。
	const [modelSearchQuery, setModelSearchQuery] = useState("");
	const [modelSearchDebounced, setModelSearchDebounced] = useState("");
	// 已从目录选中的模型 ID：应用后隐藏下拉，直到用户重新输入。
	const [appliedModelId, setAppliedModelId] = useState<string | null>(null);
	// 待确认候选跳转手动添加时携带的目录建议：右上角徽章 + 参数预填来源。
	const [pendingSuggest, setPendingSuggest] = useState<{
		remoteId: string;
		suggestions: CatalogSuggestion[];
	} | null>(null);
	const [activeSuggestIndex, setActiveSuggestIndex] = useState(0);
	// 自动添加候选搜索：关键词 + 下拉开合 + 定位高亮（点击结果滚动到候选卡）。
	const [candidateQuery, setCandidateQuery] = useState("");
	const [candidateSearchOpen, setCandidateSearchOpen] = useState(false);
	const [highlightId, setHighlightId] = useState<string | null>(null);
	const candidateSearchRef = useRef<HTMLDivElement>(null);
	const scrollAreaRef = useRef<HTMLDivElement>(null);
	const highlightTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
	// 尝试刷新失败时用弹窗展示完整错误详情（上游报错信息较长，toast 展示不完整）。
	const [refreshError, setRefreshError] = useState<string | null>(null);

	const { data: catalogHits } = useCatalogSearch(modelSearchDebounced);

	useEffect(() => {
		const closeCatalog = (event: PointerEvent) => {
			if (!catalogRef.current?.contains(event.target as Node)) setCatalogOpen(false);
		};
		document.addEventListener("pointerdown", closeCatalog);
		return () => document.removeEventListener("pointerdown", closeCatalog);
	}, []);

	useEffect(() => {
		const closeCandidateSearch = (event: PointerEvent) => {
			if (!candidateSearchRef.current?.contains(event.target as Node)) {
				setCandidateSearchOpen(false);
			}
		};
		document.addEventListener("pointerdown", closeCandidateSearch);
		return () => document.removeEventListener("pointerdown", closeCandidateSearch);
	}, []);

	useEffect(
		() => () => {
			if (highlightTimer.current) clearTimeout(highlightTimer.current);
		},
		[],
	);

	// 弹窗打开时清空联想与待确认预填。
	useEffect(() => {
		if (!open) return;
		setModelSearchQuery("");
		setModelSearchDebounced("");
		setAppliedModelId(null);
		setPendingSuggest(null);
		setCandidateQuery("");
		setCandidateSearchOpen(false);
		setActiveTab("auto");
		setCatalogOpen(false);
	}, [open]);

	// 输入时立即搜索，用户重新输入时恢复联想。
	const handleModelIdChange = (value: string) => {
		if (appliedModelId) setAppliedModelId(null);
		setCatalogOpen(true);
		setModelSearchQuery(value);
		setModelSearchDebounced(value);
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
		setCatalogOpen(false);
		setPendingSuggest(null);
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
		setPendingSuggest(null);
		setHighlightId(null);
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
				// 全部不预选，由用户自行勾选；候选全量更新后作废旧搜索。
				setSelected(new Set());
				setCandidateQuery("");
				setCandidateSearchOpen(false);
				setHighlightId(null);
			},
			onError: (error) => setRefreshError(error.message),
		});
	};

	// 候选搜索结果：对已刷新候选按模型 ID 小写子串匹配，取前 8 条。
	const candidateHits = useMemo(() => {
		const query = candidateQuery.trim().toLowerCase();
		if (!query) return [];
		return (candidates ?? [])
			.filter((candidate) => candidate.providerModelId.toLowerCase().includes(query))
			.slice(0, 8);
	}, [candidates, candidateQuery]);

	/** 点击搜索结果：滚动定位到候选卡并短暂高亮。 */
	const locateCandidate = (modelId: string) => {
		setCandidateSearchOpen(false);
		const card = scrollAreaRef.current?.querySelector<HTMLElement>(
			`[data-model-id="${CSS.escape(modelId)}"]`,
		);
		card?.scrollIntoView({ behavior: "smooth", block: "center" });
		if (highlightTimer.current) clearTimeout(highlightTimer.current);
		setHighlightId(modelId);
		highlightTimer.current = setTimeout(() => setHighlightId(null), 1800);
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
				// 添加时不配置模型级代理（默认关闭，添加后到详情编辑）。
				proxyEnabled: false,
				proxyAddr: "",
			}));
		if (list.some((item) => item.contextLength <= 0 || item.maxOutputTokens <= 0)) {
			toastError(t("common.addFailed"), new Error(t("providerModels.batchIncomplete")));
			return;
		}
		batchCreate.mutate(
			{ models: list },
			{
				onSuccess: (created) => {
					onOpenChange(false);
					toastSuccess(t("providerModels.addedCount", { count: created.length }));
				},
				onError: (error) => toastError(t("common.addFailed"), error),
			},
		);
	};

	// 手动添加表单的引用：manual 候选点击后滚动到此处并聚焦输入。
	const manualFormRef = useRef<HTMLFormElement | null>(null);

	/** 滚动到手动添加表单并聚焦模型 ID 输入。 */
	const focusManualForm = () => {
		requestAnimationFrame(() => {
			manualFormRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
			const input = manualFormRef.current?.querySelector<HTMLInputElement>(
				"input[name='providerModelId']",
			);
			input?.focus();
		});
	};

	/** 把目录建议的参数预填进表单（模型 ID 之外的全部字段）。 */
	const applySuggestion = (s: CatalogSuggestion) => {
		form.setValue("contextLength", s.contextLength ?? 0);
		form.setValue("maxOutputTokens", s.maxOutputTokens ?? 0);
		form.setValue("reasoning", s.reasoning);
		form.setValue("toolUse", s.toolUse);
		form.setValue("imageUnderstand", s.imageUnderstand);
		form.setValue("videoUnderstand", s.videoUnderstand);
	};

	/** manual（需手动填写）候选：预填模型 ID 并滚动/聚焦到手动添加表单。 */
	const jumpToManual = (candidate: RefreshCandidate) => {
		form.setValue("providerModelId", candidate.providerModelId);
		setModelSearchQuery(candidate.providerModelId);
		setModelSearchDebounced("");
		setAppliedModelId(candidate.providerModelId);
		setCatalogOpen(false);
		setActiveTab("manual");
		focusManualForm();
	};

	/** pending（待确认）候选：模型 ID 填远程 ID，参数默认按相似度最高的建议预填。 */
	const jumpToManualWithSuggestion = (candidate: RefreshCandidate) => {
		const suggestions = candidate.suggestions ?? [];
		const top = suggestions[0];
		if (top) applySuggestion(top);
		form.setValue("providerModelId", candidate.providerModelId);
		setModelSearchQuery(candidate.providerModelId);
		setModelSearchDebounced("");
		setAppliedModelId(candidate.providerModelId);
		setCatalogOpen(false);
		setPendingSuggest({ remoteId: candidate.providerModelId, suggestions });
		setActiveSuggestIndex(0);
		setActiveTab("manual");
		focusManualForm();
	};

	/** 点击相似模型徽章：切换参数预填来源，模型 ID 保持远程 ID 不变。 */
	const selectSuggestion = (index: number) => {
		const suggestion = pendingSuggest?.suggestions[index];
		if (!suggestion) return;
		applySuggestion(suggestion);
		setActiveSuggestIndex(index);
	};

	const handleManualAdd = (values: ManualFormValues) => {
		if (!provider) return;
		createModel.mutate(
			{
				...values,
				providerModelId: values.providerModelId.trim(),
				// 添加时不配置模型级代理（默认关闭，添加后到详情编辑）。
				proxyEnabled: false,
				proxyAddr: "",
			},
			{
				onSuccess: () => {
					toastSuccess(t("common.addSuccess"));
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
					setPendingSuggest(null);
				},
				onError: (error) => toastError(t("common.addFailed"), error),
			},
		);
	};

	if (!provider) return null;

	const selectedCount = selected.size;

	return (
		<>
			<Dialog open={open} onOpenChange={onOpenChange}>
				<DialogContent className="flex h-[min(720px,85vh)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[760px]">
					<DialogHeader className="shrink-0 space-y-3 px-6 pb-4 pt-6">
						<DialogTitle>{t("providerModels.addTitle")}</DialogTitle>
						<DialogDescription>
							{t("providerModels.addDesc", { provider: provider.name })}
						</DialogDescription>
					</DialogHeader>

					<div
						role="tablist"
						aria-label={t("providerModels.addTitle")}
						className="flex shrink-0 gap-1 border-b px-6"
					>
						<Button
							type="button"
							role="tab"
							variant="ghost"
							size="sm"
							aria-selected={activeTab === "auto"}
							onClick={() => setActiveTab("auto")}
						>
							{t("providerModels.autoAdd")}
						</Button>
						<Button
							type="button"
							role="tab"
							variant="ghost"
							size="sm"
							aria-selected={activeTab === "manual"}
							onClick={() => setActiveTab("manual")}
						>
							{t("providerModels.manualAdd")}
						</Button>
					</div>

					<div
						ref={scrollAreaRef}
						data-testid="add-provider-models-scroll-area"
						className="min-h-0 flex-1 overflow-y-auto px-6 py-4"
					>
						{activeTab === "auto" && (
							<div className="space-y-4">
								<div className="flex items-center justify-between gap-4">
									<Button
										type="button"
										variant="outline"
										size="sm"
										onClick={handleRefresh}
										disabled={refresh.isPending}
									>
										<RefreshCw
											className={refresh.isPending ? "mr-2 size-4 animate-spin" : "mr-2 size-4"}
										/>
										{t("providerModels.tryRefresh")}
									</Button>
									<div ref={candidateSearchRef} className="relative w-full sm:w-72">
										<Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
										<Input
											type="search"
											value={candidateQuery}
											onChange={(event) => {
												setCandidateQuery(event.target.value);
												setCandidateSearchOpen(true);
											}}
											onFocus={() => setCandidateSearchOpen(true)}
											placeholder={t("providerModels.candidateSearch")}
											aria-label={t("providerModels.candidateSearch")}
											className="pl-9"
										/>
										{candidateSearchOpen && candidateQuery.trim() && (
											<div className="absolute right-0 top-full z-20 mt-2 max-h-80 w-full overflow-y-auto rounded-xl border bg-popover p-2 shadow-lg">
												{candidateHits.length === 0 ? (
													<p className="px-3 py-2 text-sm text-muted-foreground">
														{t("providerModels.searchNoResults")}
													</p>
												) : (
													<div data-testid="candidate-search-results">
														{candidateHits.map((hit) => (
															<button
																key={hit.providerModelId}
																type="button"
																onClick={() => locateCandidate(hit.providerModelId)}
																className="flex w-full items-center justify-between gap-2 rounded-lg px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
															>
																<span className="min-w-0 truncate font-mono">
																	{hit.providerModelId}
																</span>
																<MatchStateLabel state={hit.matchState} />
															</button>
														))}
													</div>
												)}
											</div>
										)}
									</div>
								</div>

								{candidates === null ? (
									<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
										{t("providerModels.notRefreshed")}
									</div>
								) : candidates.length === 0 ? (
									<div className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
										{t("providerModels.refreshEmpty")}
									</div>
								) : (
									<div className="space-y-3">
										<div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
											{candidates.map((candidate) => {
												const edits = editsOf(candidate);
												const selectable = isSelectable(candidate);
												// manual/pending 候选整卡可点：跳转手动添加表单（pending 额外预填建议参数）。
												const clickable =
													candidate.matchState === "manual" || candidate.matchState === "pending";
												const jump = () =>
													candidate.matchState === "pending"
														? jumpToManualWithSuggestion(candidate)
														: jumpToManual(candidate);
												return (
													<div
														key={candidate.providerModelId}
														data-model-id={candidate.providerModelId}
														className={cn(
															"rounded-lg border p-3 transition-colors",
															clickable &&
																"cursor-pointer hover:border-primary/50 hover:bg-muted/40",
															candidate.providerModelId === highlightId &&
																"border-primary ring-2 ring-primary/40",
														)}
														role={clickable ? "button" : undefined}
														tabIndex={clickable ? 0 : undefined}
														onClick={clickable ? jump : undefined}
														onKeyDown={
															clickable
																? (e) => {
																		if (e.key === "Enter" || e.key === " ") {
																			e.preventDefault();
																			jump();
																		}
																	}
																: undefined
														}
													>
														<div className="flex items-center gap-2.5">
															<Checkbox
																checked={selected.has(candidate.providerModelId)}
																disabled={!selectable}
																onClick={(event) => event.stopPropagation()}
																onCheckedChange={(checked) =>
																	toggleSelect(candidate, checked === true)
																}
																aria-label={`${t("providerModels.selectModel")} ${candidate.providerModelId}`}
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
																<Label className="text-xs text-muted-foreground">
																	{t("providerModels.contextLength")}
																</Label>
																<Input
																	type="number"
																	min={1}
																	className="h-8"
																	value={edits.contextLength}
																	placeholder={t("providerModels.required")}
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
																<Label className="text-xs text-muted-foreground">
																	{t("providerModels.maxOutput")}
																</Label>
																<Input
																	type="number"
																	min={1}
																	className="h-8"
																	value={edits.maxOutputTokens}
																	placeholder={t("providerModels.required")}
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
										<span className="text-xs text-muted-foreground">
											{t("providerModels.selectedOfTotal", {
												selected: selectedCount,
												total: candidates.length,
											})}
										</span>
									</div>
								)}
							</div>
						)}

						{activeTab === "manual" && (
							<Form {...form}>
								<form
									id="provider-model-manual-form"
									ref={manualFormRef}
									onSubmit={form.handleSubmit(handleManualAdd)}
									className="space-y-4"
								>
									<div className="flex items-center justify-between gap-2">
										<h3 className="text-sm font-semibold">{t("providerModels.manualAdd")}</h3>
										{pendingSuggest !== null && pendingSuggest.suggestions.length > 0 && (
											<div
												className="flex items-center gap-1.5"
												aria-label={t("providerModels.similarModels")}
											>
												{pendingSuggest.suggestions.map((suggestion, index) => (
													<button
														key={suggestion.catalogId}
														type="button"
														title={suggestion.catalogId}
														aria-pressed={index === activeSuggestIndex}
														onClick={() => selectSuggestion(index)}
														className={cn(
															badgeVariants({
																variant: index === activeSuggestIndex ? "default" : "outline",
															}),
															"max-w-44 cursor-pointer font-mono",
														)}
													>
														<span className="truncate">{suggestion.catalogId}</span>
													</button>
												))}
											</div>
										)}
									</div>
									<FormField
										control={form.control}
										name="providerModelId"
										render={({ field }) => (
											<FormItem ref={catalogRef} className="relative">
												<FormLabel required>{t("providerModels.modelId")}</FormLabel>
												<FormControl>
													<Input
														placeholder={t("providerModels.modelIdPlaceholder")}
														{...field}
														value={modelSearchQuery}
														onFocus={() => setCatalogOpen(true)}
														onChange={(e) => {
															field.onChange(e);
															handleModelIdChange(e.target.value);
														}}
													/>
												</FormControl>
												{/* 关键词搜索联想下拉：点击候选自动填充全部字段；应用后隐藏 */}
												{catalogOpen &&
													(catalogHits?.length ?? 0) > 0 &&
													modelSearchQuery.trim().length > 0 &&
													!appliedModelId && (
														<div className="absolute inset-x-0 top-full z-20 mt-1 max-h-64 overflow-y-auto rounded-lg border border-input bg-popover p-1 shadow-lg backdrop-blur-xl">
															{catalogHits?.map((hit) => (
																<button
																	key={hit.id}
																	type="button"
																	aria-label={hit.id}
																	onClick={() => applyCatalogCandidate(hit)}
																	className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-muted/60"
																>
																	<Sparkles className="size-4 shrink-0 text-success" />
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
													<FormLabel required>{t("providerModels.contextLength")}</FormLabel>
													<FormControl>
														<Input
															type="number"
															min={1}
															placeholder={t("providerModels.contextPlaceholder")}
															{...field}
														/>
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
													<FormLabel required>{t("providerModels.maxOutput")}</FormLabel>
													<FormControl>
														<Input
															type="number"
															min={1}
															placeholder={t("providerModels.maxOutputPlaceholder")}
															{...field}
														/>
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
														<FormLabel>{t(CAPABILITY_LABEL_KEYS[key])}</FormLabel>
														<FormControl>
															<Switch checked={field.value} onCheckedChange={field.onChange} />
														</FormControl>
													</FormItem>
												)}
											/>
										))}
									</div>
								</form>
							</Form>
						)}
					</div>
					<DialogFooter className="shrink-0 border-t px-6 py-4">
						{activeTab === "auto" && (
							<Button
								type="button"
								size="sm"
								onClick={handleBatchAdd}
								disabled={selectedCount === 0 || batchCreate.isPending}
							>
								{t("providerModels.addSelected")}
							</Button>
						)}
						{activeTab === "manual" && (
							<Button
								type="submit"
								size="sm"
								form="provider-model-manual-form"
								disabled={createModel.isPending}
							>
								{t("providerModels.manualAddButton")}
							</Button>
						)}
					</DialogFooter>
				</DialogContent>
			</Dialog>

			<ConfirmDialog
				open={refreshError !== null}
				onOpenChange={(open) => {
					if (!open) setRefreshError(null);
				}}
				title={t("providerModels.refreshFailed")}
				desc={
					<>
						<p>{t("providerModels.refreshFailedDesc")}</p>
						<p className="mt-2 max-h-48 overflow-y-auto rounded-lg bg-muted p-3 font-mono text-xs text-destructive whitespace-pre-wrap break-all">
							{refreshError}
						</p>
					</>
				}
				confirmText={t("providerModels.close")}
				handleConfirm={() => setRefreshError(null)}
			/>
		</>
	);
}
