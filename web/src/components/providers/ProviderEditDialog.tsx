import { BILLING_MODES, PROTOCOL_TYPES } from "@/components/providers/ProtocolIcon";
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
import { Label } from "@/components/ui/label";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
	type Provider,
	type ProviderTemplate,
	useCreateProvider,
	useMatchTemplate,
	useUpdateProvider,
} from "@/hooks/use-providers";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { ChevronDown, ChevronRight, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

function makeFormSchema(t: (key: string) => string) {
	return z
		.object({
			name: z.string().min(1, t("providers.nameRequired")),
			baseUrl: z.string().min(1, t("providers.baseUrlRequired")),
			// apiKey 创建时必填（编辑时留空表示不修改，在 onSubmit 里按模式校验）。
			apiKey: z.string(),
			enable: z.boolean(),
			// 枚举范围在 onSubmit 里校验，避免 zod refine 把类型收窄成字面量联合。
			protocolType: z.number(),
			billingMode: z.number(),
			usageEnabled: z.boolean(),
			customHeader: z.string().refine((v) => isJsonObject(v), t("providers.invalidCustomHeader")),
			// 网络代理：开启时地址必填且需 http:// 开头。
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

/** 解析模板 extra：仅当其为合法 JSON 对象时返回键值映射，否则空。 */
function parseExtra(extra: string | undefined): Record<string, unknown> {
	if (!extra) return {};
	try {
		const parsed: unknown = JSON.parse(extra);
		return parsed && typeof parsed === "object" && !Array.isArray(parsed)
			? (parsed as Record<string, unknown>)
			: {};
	} catch {
		return {};
	}
}

function isJsonObject(value: string): boolean {
	if (!value.trim()) return true;
	try {
		const parsed: unknown = JSON.parse(value);
		return !!parsed && typeof parsed === "object" && !Array.isArray(parsed);
	} catch {
		return false;
	}
}

/** 需要用户填写的 extra 字段：排除标记字段与后端维护的派生凭据。 */
function editableExtraKeys(extra: string | undefined): string[] {
	const map = parseExtra(extra);
	return Object.keys(map).filter((k) => k !== "usage" && k !== "usage_type" && k !== "jwt");
}

/** 模板 extra 中值为 true 的 usage 标记。 */
function usageFlag(extra: string | undefined): boolean {
	return parseExtra(extra).usage === true;
}

/** 用量开启后校验：extra 中所有非空模板字段都必须填写，返回缺失字段列表。 */
function missingRequiredExtra(
	extraValues: Record<string, string>,
	templateExtra: string | undefined,
): string[] {
	return editableExtraKeys(templateExtra).filter((k) => !extraValues[k]?.trim());
}

interface ProviderEditDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	/** 非空 = 编辑模式；null = 创建模式。 */
	provider: Provider | null;
}

export function ProviderEditDialog({ open, onOpenChange, provider }: ProviderEditDialogProps) {
	const isEdit = provider !== null;
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const createProvider = useCreateProvider();
	const updateProvider = useUpdateProvider();
	const formSchema = useMemo(() => makeFormSchema(t), [t]);

	const [advancedOpen, setAdvancedOpen] = useState(false);
	const [extraValues, setExtraValues] = useState<Record<string, string>>({});
	// 模板 extra 中需要填写的字段清单（不含标记与后端派生凭据）。
	const [extraKeys, setExtraKeys] = useState<string[]>([]);

	const form = useForm<FormValues>({
		resolver: zodResolver(formSchema),
		defaultValues: {
			name: "",
			baseUrl: "",
			apiKey: "",
			enable: true,
			protocolType: 0,
			billingMode: 0,
			usageEnabled: false,
			customHeader: "{}",
			proxyEnabled: false,
			proxyAddr: "",
		},
	});

	const baseUrl = form.watch("baseUrl");
	const usageEnabled = form.watch("usageEnabled");

	const { data: matchedTemplates } = useMatchTemplate(isEdit || !open ? "" : baseUrl);

	// 已应用的模板（用户点击「使用 XXX 模板」后设置）；应用后下拉框消失。
	const [appliedTemplate, setAppliedTemplate] = useState<ProviderTemplate | null>(null);

	// 当前生效的 extra：编辑模式用已有值；创建模式用已应用的模板值。
	const templateExtra = useMemo(
		() => (isEdit ? provider?.extra : appliedTemplate?.extra),
		[isEdit, provider?.extra, appliedTemplate?.extra],
	);

	// 打开弹窗时初始化表单与 extra 字段。
	useEffect(() => {
		if (!open) return;
		form.reset({
			name: provider?.name ?? "",
			baseUrl: provider?.baseUrl ?? "",
			apiKey: "",
			enable: provider?.enable ?? true,
			// 编辑用已有值；创建默认 OpenAI Compatible / 按量付费。
			protocolType: provider?.protocolType ?? 0,
			billingMode: provider?.billingMode ?? 0,
			usageEnabled: usageFlag(provider?.extra),
			customHeader: provider?.customHeader ?? "{}",
			proxyEnabled: provider?.proxyEnabled ?? false,
			proxyAddr: provider?.proxyAddr ?? "",
		});
		const keys = editableExtraKeys(isEdit ? provider?.extra : undefined);
		const defaults: Record<string, string> = {};
		for (const key of keys) {
			const value = parseExtra(provider?.extra)[key];
			defaults[key] = typeof value === "string" ? value : "";
		}
		setExtraKeys(keys);
		setExtraValues({ ...defaults });
		setAdvancedOpen(false);
		setAppliedTemplate(null);
	}, [open, provider, isEdit, form]);

	// 用户点击「使用 XXX 模板」后，按模板内容填充表单（名称覆盖、协议/付费、extra 字段）。
	const applyTemplate = (template: ProviderTemplate) => {
		form.setValue("name", template.name);
		form.setValue("protocolType", template.protocolType);
		form.setValue("billingMode", template.billingMode);
		const keys = editableExtraKeys(template.extra);
		const defaults: Record<string, string> = {};
		for (const key of keys) {
			defaults[key] = "";
		}
		setExtraKeys(keys);
		setExtraValues({ ...defaults });
		form.setValue("usageEnabled", usageFlag(template.extra));
		setAppliedTemplate(template);
	};

	const submitLabel = isEdit ? t("common.save") : t("common.create");

	const onSubmit = (values: FormValues) => {
		// 编辑模式：API Key 留空 = 不修改；创建模式：必须填写。
		if (!isEdit && !values.apiKey.trim()) {
			toastError(t("providers.submitFailed"), new Error(t("providers.apiKeyRequired")));
			return;
		}
		// 协议类型/付费类型枚举范围（与后端校验一致）。
		if (values.protocolType < 0 || values.protocolType > 3) {
			toastError(t("providers.submitFailed"), new Error(t("providers.invalidProtocol")));
			return;
		}
		if (values.billingMode !== 0 && values.billingMode !== 1) {
			toastError(t("providers.submitFailed"), new Error(t("providers.invalidBilling")));
			return;
		}
		// 用量开关开启时，模板推荐字段必须全部填写。
		if (values.usageEnabled) {
			const missing = missingRequiredExtra(extraValues, templateExtra);
			if (missing.length > 0) {
				toastError(
					t("providers.submitFailed"),
					new Error(`${t("providers.usageMissingFields")}${missing.join("、")}`),
				);
				return;
			}
		}

		const payload = {
			name: values.name.trim(),
			enable: values.enable,
			baseUrl: values.baseUrl.trim(),
			apiKey: values.apiKey.trim(),
			protocolType: values.protocolType,
			billingMode: values.billingMode,
			customHeader: values.customHeader.trim() || "{}",
			// 网络代理：开启时地址必填（表单已校验）；未开启地址清空。
			proxyEnabled: values.proxyEnabled,
			proxyAddr: values.proxyEnabled ? values.proxyAddr.trim() : "",
			// 合并模板 extra 与用户填写值；usage 标记由用量开关驱动。
			extra: JSON.stringify({
				...(templateExtra ? parseExtra(templateExtra) : {}),
				...extraValues,
				...(templateExtra && usageFlag(templateExtra) ? { usage: values.usageEnabled } : {}),
			}),
		};

		const onSuccess = () => {
			onOpenChange(false);
			toastSuccess(isEdit ? t("common.updateSuccess") : t("common.createSuccess"));
		};
		const onError = (error: Error) => {
			toastError(isEdit ? t("common.updateFailed") : t("common.createFailed"), error);
		};

		if (isEdit) {
			updateProvider.mutate({ id: provider.id, ...payload }, { onSuccess, onError });
		} else {
			createProvider.mutate(payload, { onSuccess, onError });
		}
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-[560px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>
						{isEdit ? t("providers.editTitle") : t("providers.createTitle")}
					</DialogTitle>
					<DialogDescription>
						{isEdit ? t("providers.editDesc") : t("providers.createDesc")}
					</DialogDescription>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
						<FormField
							control={form.control}
							name="baseUrl"
							render={({ field }) => (
								<FormItem className="relative">
									<FormLabel required>Base URL</FormLabel>
									<FormControl>
										<Input placeholder="https://api.deepseek.com" {...field} />
									</FormControl>
									{/* 搜索框联想下拉：匹配到模板后，输入框下方浮出候选列表，点击某项即应用 */}
									{!isEdit && !appliedTemplate && (matchedTemplates?.length ?? 0) > 0 && (
										<div className="absolute inset-x-0 top-full z-20 mt-1 max-h-64 overflow-y-auto rounded-lg border border-input bg-popover p-1 shadow-lg backdrop-blur-xl">
											{matchedTemplates?.map((template) => (
												<button
													key={template.name}
													type="button"
													onClick={() => applyTemplate(template)}
													className="flex w-full items-center gap-2 rounded-md px-3 py-2.5 text-left text-sm transition-colors hover:bg-muted/60"
												>
													<Sparkles className="size-4 shrink-0 text-success" />
													<span className="truncate">
														{t("providers.applyTemplate")}{" "}
														<span className="font-medium">{template.name}</span>{" "}
														{t("providers.templateSuffix")}
													</span>
												</button>
											))}
										</div>
									)}
									<FormMessage />
								</FormItem>
							)}
						/>
						<FormField
							control={form.control}
							name="name"
							render={({ field }) => (
								<FormItem>
									<FormLabel required>{t("providers.name")}</FormLabel>
									<FormControl>
										<Input placeholder={t("providers.namePlaceholder")} {...field} />
									</FormControl>
									<FormMessage />
								</FormItem>
							)}
						/>
						<FormField
							control={form.control}
							name="apiKey"
							render={({ field }) => (
								<FormItem>
									<FormLabel required>{t("providers.apiKey")}</FormLabel>
									<FormControl>
										<Input
											type="password"
											placeholder={isEdit ? t("providers.leaveEmptyHint") : "sk-..."}
											autoComplete="off"
											{...field}
										/>
									</FormControl>
									<FormMessage />
								</FormItem>
							)}
						/>
						<div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
							<FormField
								control={form.control}
								name="protocolType"
								render={({ field }) => (
									<FormItem>
										<FormLabel>{t("providers.protocolType")}</FormLabel>
										<Select
											value={String(field.value)}
											onValueChange={(v) => field.onChange(Number(v))}
										>
											<FormControl>
												<SelectTrigger>
													<SelectValue placeholder={t("providers.selectProtocol")} />
												</SelectTrigger>
											</FormControl>
											<SelectContent>
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
							<FormField
								control={form.control}
								name="billingMode"
								render={({ field }) => (
									<FormItem>
										<FormLabel>{t("providers.billingMode")}</FormLabel>
										<Select
											value={String(field.value)}
											onValueChange={(v) => field.onChange(Number(v))}
										>
											<FormControl>
												<SelectTrigger>
													<SelectValue placeholder={t("providers.selectBilling")} />
												</SelectTrigger>
											</FormControl>
											<SelectContent>
												{BILLING_MODES.map((b) => (
													<SelectItem key={b.value} value={String(b.value)}>
														{t(b.labelKey)}
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
									<FormLabel>{t("providers.enable")}</FormLabel>
									<FormControl>
										<Switch checked={field.value} onCheckedChange={field.onChange} />
									</FormControl>
								</FormItem>
							)}
						/>

						{/* 网络代理：仅当启用时显示地址输入。 */}
						<ProxyConfigFields control={form.control} withHint />

						{/* 高级设置：常驻展示，默认折叠。 */}
						<div className="overflow-hidden rounded-lg border">
							<button
								type="button"
								onClick={() => setAdvancedOpen((v) => !v)}
								className="flex w-full items-center justify-between px-4 py-3 text-sm font-medium transition-colors hover:bg-foreground/5"
							>
								<span>{t("providers.advanced")}</span>
								{advancedOpen ? (
									<ChevronDown className="size-4 text-muted-foreground" />
								) : (
									<ChevronRight className="size-4 text-muted-foreground" />
								)}
							</button>
							{advancedOpen && (
								<div className="space-y-4 border-t px-4 py-4">
									{/* 自定义请求头置于高级设置最上方 */}
									<div className="space-y-1.5">
										<Label htmlFor="custom-header">{t("providers.customHeaderJson")}</Label>
										<FormField
											control={form.control}
											name="customHeader"
											render={({ field }) => (
												<FormItem>
													<FormControl>
														<textarea
															id="custom-header"
															rows={2}
															className="flex w-full rounded-lg border border-input bg-white/70 px-3 py-2 font-mono text-xs shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/12 dark:bg-white/5 dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.05)]"
															placeholder='{"X-Api-Key": "..."}'
															{...field}
														/>
													</FormControl>
													<FormMessage />
												</FormItem>
											)}
										/>
									</div>
									{/* 用量展示开关：仅模板支持用量查询时展示 */}
									{templateExtra && usageFlag(templateExtra) && (
										<FormField
											control={form.control}
											name="usageEnabled"
											render={({ field }) => (
												<FormItem className="flex items-center justify-between rounded-lg border p-3">
													<FormLabel>{t("providers.usageDisplay")}</FormLabel>
													<FormControl>
														<Switch checked={field.value} onCheckedChange={field.onChange} />
													</FormControl>
												</FormItem>
											)}
										/>
									)}
									{/* Extra 字段：仅用量展示开关开启时展示 */}
									{usageEnabled &&
										extraKeys.map((key) => (
											<div key={key} className="space-y-1.5">
												<Label htmlFor={`extra-${key}`}>{key}</Label>
												<Input
													id={`extra-${key}`}
													type={key === "password" ? "password" : "text"}
													autoComplete="off"
													value={extraValues[key] ?? ""}
													onChange={(e) =>
														setExtraValues((prev) => ({ ...prev, [key]: e.target.value }))
													}
													placeholder={`${t("providers.templateField")} ${key}`}
												/>
											</div>
										))}
								</div>
							)}
						</div>

						<DialogFooter className="gap-2 pt-2">
							<Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
								{t("common.cancel")}
							</Button>
							<Button type="submit" disabled={createProvider.isPending || updateProvider.isPending}>
								{submitLabel}
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
