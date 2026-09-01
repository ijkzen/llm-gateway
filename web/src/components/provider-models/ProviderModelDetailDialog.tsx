import { ConfirmDialog } from "@/components/confirm-dialog";
import { CAPABILITIES } from "@/components/provider-models/CapabilityIcons";
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
import { Switch } from "@/components/ui/switch";
import {
	type ProviderModel,
	useDeleteProviderModel,
	useTestProviderModel,
	useUpdateProviderModel,
} from "@/hooks/use-provider-models";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { FlaskConical, Loader2, Pencil, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

function makeFormSchema(t: (key: string) => string) {
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

type FormValues = z.infer<ReturnType<typeof makeFormSchema>>;

interface ProviderModelDetailDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	providerId: number;
	providerName: string;
	model: ProviderModel | null;
}

/** 模型详情弹窗：默认只读；「编辑」后右下角变为「删除」与「更新」，更新后回到只读。 */
export function ProviderModelDetailDialog({
	open,
	onOpenChange,
	providerId,
	providerName,
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
		});
		setEditing(false);
		setConfirmingDelete(false);
		setTestError(null);
	}, [open, model, form]);

	const onSubmit = (values: FormValues) => {
		if (!model) return;
		updateModel.mutate(
			{ modelId: model.modelId, ...values },
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
						<DialogTitle className="truncate" title={model.providerModelId}>
							{model.providerModelId}
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
													? "flex items-center gap-1.5 text-sm text-emerald-600 dark:text-emerald-400"
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
									disabled={updateModel.isPending}
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

			<ConfirmDialog
				open={testError !== null}
				onOpenChange={(open) => {
					if (!open) setTestError(null);
				}}
				title={t("providerModels.testFailedTitle")}
				desc={
					<>
						<p>{t("providerModels.testFailedDesc")}</p>
						<p className="mt-2 rounded-lg bg-muted p-3 font-mono text-xs text-destructive break-all">
							{testError}
						</p>
					</>
				}
				confirmText={t("providerModels.close")}
				handleConfirm={() => setTestError(null)}
			/>
		</>
	);
}
