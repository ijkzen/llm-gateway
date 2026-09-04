import { ConfirmDialog } from "@/components/confirm-dialog";
import { CAPABILITIES } from "@/components/provider-models/CapabilityIcons";
import { TestFailedDialog } from "@/components/provider-models/TestFailedDialog";
import { PROTOCOL_TYPES, protocolLabel } from "@/components/providers/ProtocolIcon";
import { ProviderProxyRow } from "@/components/providers/ProviderProxyRow";
import { ProxyConfigFields } from "@/components/providers/ProxyConfigFields";
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
import { Switch } from "@/components/ui/switch";
import {
	type ProviderModel,
	useDeleteProviderModel,
	useTestProviderModel,
	useUpdateProviderModel,
} from "@/hooks/use-provider-models";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { ChevronRight, FlaskConical, Loader2, Pencil, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { flushSync } from "react-dom";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";
import { z } from "zod";

function makeFormSchema(t: (key: string) => string) {
	return z
		.object({
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
			// 模型单独选择的协议：null=跟随供应商；0..=3=覆盖（与供应商协议枚举一致）。
			protocolType: z.number().int().nullable(),
			// 模型级网络代理：开启时地址必填且需 http:// 开头（与供应商代理同规则）。
			proxyEnabled: z.boolean(),
			proxyAddr: z.string(),
		})
		.superRefine((values, ctx) => {
			if (values.proxyEnabled) {
				if (!values.proxyAddr.trim()) {
					ctx.addIssue({
						code: z.ZodIssueCode.custom,
						path: ["proxyAddr"],
						message: t("providers.proxyAddrRequired"),
					});
				} else if (!values.proxyAddr.trim().startsWith("http://")) {
					ctx.addIssue({
						code: z.ZodIssueCode.custom,
						path: ["proxyAddr"],
						message: t("providers.proxyAddrInvalid"),
					});
				}
			}
		});
}

type FormValues = z.infer<ReturnType<typeof makeFormSchema>>;

interface ProviderModelDetailDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	providerId: number;
	providerName: string;
	/** 所属供应商的代理地址（模型级关闭时用于展示继承来源）。 */
	providerProxyAddr?: string;
	/** 所属供应商的协议类型（模型跟随供应商时用于展示生效协议来源）。 */
	providerProtocolType: number;
	model: ProviderModel | null;
}

/** 模型详情弹窗：默认只读；「编辑」后右下角变为「删除」与「更新」，更新后回到只读。 */
export function ProviderModelDetailDialog({
	open,
	onOpenChange,
	providerId,
	providerName,
	providerProxyAddr,
	providerProtocolType,
	model,
}: ProviderModelDetailDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateModel = useUpdateProviderModel(providerId);
	const deleteModel = useDeleteProviderModel(providerId);
	const testModel = useTestProviderModel(providerId);
	const [editing, setEditing] = useState(false);
	const [confirmingDelete, setConfirmingDelete] = useState(false);
	const [testError, setTestError] = useState<string | null>(null);
	const formSchema = useMemo(() => makeFormSchema(t), [t]);

	const form = useForm<FormValues>({
		resolver: zodResolver(formSchema),
		defaultValues: {
			providerModelId: "",
			contextLength: 0,
			maxOutputTokens: 0,
			reasoning: false,
			toolUse: false,
			imageUnderstand: false,
			videoUnderstand: false,
			protocolType: null,
			proxyEnabled: false,
			proxyAddr: "",
		},
	});

	// 打开弹窗或切换目标模型时重置表单与编辑态。
	useEffect(() => {
		if (!open || !model) return;
		form.reset({
			providerModelId: model.providerModelId,
			contextLength: model.contextLength,
			maxOutputTokens: model.maxOutputTokens,
			reasoning: model.reasoning,
			toolUse: model.toolUse,
			imageUnderstand: model.imageUnderstand,
			videoUnderstand: model.videoUnderstand,
			protocolType: model.protocolType ?? null,
			proxyEnabled: model.proxyEnabled,
			proxyAddr: model.proxyAddr,
		});
		setEditing(false);
		setConfirmingDelete(false);
		setTestError(null);
	}, [open, model, form]);

	const onSubmit = (values: FormValues) => {
		// 进入编辑态后未改动任何值不提交：防止双击「编辑」时第二击落在同槽位的「更新」上，
		// 把未变更的值原样 PUT 并误报「更新成功」。
		if (!model || !form.formState.isDirty) return;
		updateModel.mutate(
			{
				modelId: model.modelId,
				...values,
				// 关闭代理时地址清空（与供应商代理提交一致，避免残留旧地址）。
				proxyAddr: values.proxyEnabled ? values.proxyAddr.trim() : "",
			},
			{
				onSuccess: () => {
					setEditing(false);
					toastSuccess(t("common.updateSuccess"));
				},
				onError: (error) => toastError(t("common.updateFailed"), error),
			},
		);
	};

	const handleDelete = () => {
		if (!model) return;
		deleteModel.mutate(model.modelId, {
			onSuccess: () => {
				flushSync(() => setConfirmingDelete(false));
				onOpenChange(false);
				toastSuccess(t("common.deleteSuccess"));
			},
			onError: (error) => toastError(t("common.deleteFailed"), error),
		});
	};

	const handleTest = () => {
		if (!model || testModel.isPending) return;
		testModel.mutate(model.modelId, {
			onSuccess: () => toastSuccess(t("providerModels.testSuccess")),
			onError: (error) => setTestError(error.message),
		});
	};

	if (!model) return null;

	return (
		<>
			<Dialog
				open={open}
				onOpenChange={(next) => {
					if (!next) setEditing(false);
					onOpenChange(next);
				}}
			>
				<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[520px]">
					<DialogHeader className="space-y-3">
						<DialogTitle className="min-w-0">
							<Link
								to={`/models/${providerId}/${encodeURIComponent(model.providerModelId)}/overview`}
								className="group flex min-w-0 items-center justify-center gap-0.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/60 sm:justify-start"
								title={t("providerModels.viewModelOverview", { model: model.providerModelId })}
							>
								<span className="min-w-0 truncate">{model.providerModelId}</span>
								<ChevronRight className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-foreground" />
							</Link>
						</DialogTitle>
						<DialogDescription>
							{t("providerModels.belongsToProvider")}
							{providerName}
						</DialogDescription>
					</DialogHeader>

					{editing ? (
						<Form {...form}>
							<form
								id="provider-model-detail-form"
								onSubmit={form.handleSubmit(onSubmit)}
								className="space-y-4"
							>
								<FormField
									control={form.control}
									name="providerModelId"
									render={({ field }) => (
										<FormItem>
											<FormLabel required>{t("providerModels.modelId")}</FormLabel>
											<FormControl>
												<Input placeholder={t("providerModels.modelIdPlaceholder")} {...field} />
											</FormControl>
											<FormMessage />
										</FormItem>
									)}
								/>
								<FormField
									control={form.control}
									name="protocolType"
									render={({ field }) => (
										<FormItem>
											<FormLabel>{t("providerModels.protocolType")}</FormLabel>
											<Select
												value={field.value === null ? "null" : String(field.value)}
												onValueChange={(v) => field.onChange(v === "null" ? null : Number(v))}
											>
												<FormControl>
													<SelectTrigger>
														<SelectValue placeholder={t("providerModels.selectProtocol")} />
													</SelectTrigger>
												</FormControl>
												<SelectContent>
													<SelectItem value="null">{t("providerModels.followProvider")}</SelectItem>
													{PROTOCOL_TYPES.map((p) => (
														<SelectItem key={p.value} value={String(p.value)}>
															{t(p.labelKey)}
														</SelectItem>
													))}
												</SelectContent>
											</Select>
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
													<Input type="number" min={1} {...field} />
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
													<Input type="number" min={1} {...field} />
												</FormControl>
												<FormMessage />
											</FormItem>
										)}
									/>
								</div>
								<div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
									{CAPABILITIES.map(({ key, labelKey }) => (
										<FormField
											key={key}
											control={form.control}
											name={key}
											render={({ field }) => (
												<FormItem className="flex items-center justify-between rounded-lg border p-3">
													<FormLabel>{t(labelKey)}</FormLabel>
													<FormControl>
														<Switch checked={field.value} onCheckedChange={field.onChange} />
													</FormControl>
												</FormItem>
											)}
										/>
									))}
								</div>

								{/* 模型级网络代理：开关 + 条件显示地址输入（优先于供应商代理）。 */}
								<ProxyConfigFields control={form.control} />
							</form>
						</Form>
					) : (
						<dl className="space-y-3">
							<div className="flex items-center justify-between gap-4 rounded-lg border px-4 py-2.5">
								<dt className="text-sm text-muted-foreground">{t("providerModels.modelId")}</dt>
								<dd className="min-w-0 truncate font-mono text-sm">{model.providerModelId}</dd>
							</div>
							<div className="grid grid-cols-2 gap-3">
								<div className="rounded-lg border px-4 py-2.5">
									<dt className="text-xs text-muted-foreground">
										{t("providerModels.contextLength")}
									</dt>
									<dd className="mt-0.5 text-sm font-medium">
										{model.contextLength.toLocaleString()}
									</dd>
								</div>
								<div className="rounded-lg border px-4 py-2.5">
									<dt className="text-xs text-muted-foreground">{t("providerModels.maxOutput")}</dt>
									<dd className="mt-0.5 text-sm font-medium">
										{model.maxOutputTokens.toLocaleString()}
									</dd>
								</div>
							</div>
							<div className="rounded-lg border px-4 py-3">
								<dt className="text-xs text-muted-foreground">
									{t("providerModels.modelCapabilities")}
								</dt>
								<dd className="mt-2 grid grid-cols-2 gap-2">
									{CAPABILITIES.map(({ key, labelKey, icon: Icon }) => (
										<span
											key={key}
											className={
												model[key]
													? "flex items-center gap-1.5 text-sm text-success"
													: "flex items-center gap-1.5 text-sm text-muted-foreground/60"
											}
										>
											<Icon className="size-3.5" />
											{t(labelKey)}
											{model[key]
												? t("providerModels.supported")
												: t("providerModels.notSupported")}
										</span>
									))}
								</dd>
							</div>
							<div className="flex items-center justify-between rounded-lg border px-4 py-2.5">
								<dt className="text-sm text-muted-foreground">
									{t("providerModels.protocolType")}
								</dt>
								<dd className="text-sm font-medium">
									{model.protocolType !== null
										? // 模型单独指定了协议：直接显示该协议名。
											protocolLabel(model.protocolType)
										: // 跟随供应商：显示「跟随供应商（供应商协议名）」。
											t("providerModels.followProviderWith", {
												protocol: protocolLabel(providerProtocolType),
											})}
								</dd>
							</div>
							<div className="flex items-center justify-between rounded-lg border px-4 py-2.5">
								<dt className="text-sm text-muted-foreground">{t("providers.proxyEnabled")}</dt>
								<dd>
									<ProviderProxyRow
										enabled={model.proxyEnabled}
										addr={model.proxyAddr}
										inherited={providerProxyAddr}
									/>
								</dd>
							</div>
						</dl>
					)}

					<DialogFooter className="gap-2 pt-2">
						{editing ? (
							<>
								<Button
									type="button"
									variant="outline"
									size="sm"
									className="text-destructive hover:text-destructive"
									onClick={() => setConfirmingDelete(true)}
								>
									<Trash2 className="mr-1.5 size-4" />
									{t("providerModels.deleteModel")}
								</Button>
								<Button
									type="submit"
									size="sm"
									form="provider-model-detail-form"
									disabled={updateModel.isPending || !form.formState.isDirty}
								>
									{t("providerModels.update")}
								</Button>
							</>
						) : (
							<>
								<Button
									type="button"
									variant="outline"
									size="sm"
									onClick={handleTest}
									disabled={testModel.isPending}
								>
									{testModel.isPending ? (
										<Loader2 className="mr-1.5 size-4 animate-spin" />
									) : (
										<FlaskConical className="mr-1.5 size-4" />
									)}
									{t(testModel.isPending ? "providerModels.testing" : "providerModels.test")}
								</Button>
								<Button type="button" variant="outline" size="sm" onClick={() => setEditing(true)}>
									<Pencil className="mr-1.5 size-4" />
									{t("providerModels.editModel")}
								</Button>
							</>
						)}
					</DialogFooter>
				</DialogContent>
			</Dialog>

			<ConfirmDialog
				open={confirmingDelete}
				onOpenChange={setConfirmingDelete}
				title={t("providerModels.deleteTitle")}
				desc={
					<>
						{t("providerModels.deleteDesc")}{" "}
						<span className="font-semibold">{model.providerModelId}</span>{" "}
						{t("providerModels.deleteDescSuffix")}
					</>
				}
				confirmText={t("common.delete")}
				destructive
				isLoading={deleteModel.isPending}
				handleConfirm={handleDelete}
			/>

			<TestFailedDialog message={testError} onClose={() => setTestError(null)} />
		</>
	);
}
