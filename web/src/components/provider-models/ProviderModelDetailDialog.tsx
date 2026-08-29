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
	useUpdateProviderModel,
} from "@/hooks/use-provider-models";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { Pencil, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const formSchema = z.object({
	providerModelId: z.string().min(1, "模型 ID 不能为空"),
	contextLength: z.coerce.number().int("必须为整数").positive("必须为正整数"),
	maxOutputTokens: z.coerce.number().int("必须为整数").positive("必须为正整数"),
	reasoning: z.boolean(),
	toolUse: z.boolean(),
	imageUnderstand: z.boolean(),
	videoUnderstand: z.boolean(),
});

type FormValues = z.infer<typeof formSchema>;

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
	const { toastSuccess, toastError } = useToastActions();
	const updateModel = useUpdateProviderModel(providerId);
	const deleteModel = useDeleteProviderModel(providerId);
	const [editing, setEditing] = useState(false);
	const [confirmingDelete, setConfirmingDelete] = useState(false);

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
	}, [open, model, form]);

	const onSubmit = (values: FormValues) => {
		if (!model) return;
		updateModel.mutate(
			{ modelId: model.modelId, ...values },
			{
				onSuccess: () => {
					setEditing(false);
					toastSuccess("更新成功");
				},
				onError: (error) => toastError("更新失败", error),
			},
		);
	};

	const handleDelete = () => {
		if (!model) return;
		deleteModel.mutate(model.modelId, {
			onSuccess: () => {
				onOpenChange(false);
				toastSuccess("删除成功");
			},
			onError: (error) => toastError("删除失败", error),
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
						<DialogDescription>所属供应商：{providerName}</DialogDescription>
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
										name="contextLength"
										render={({ field }) => (
											<FormItem>
												<FormLabel required>上下文长度</FormLabel>
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
												<FormLabel required>最大输出</FormLabel>
												<FormControl>
													<Input type="number" min={1} {...field} />
												</FormControl>
												<FormMessage />
											</FormItem>
										)}
									/>
								</div>
								<div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
									{CAPABILITIES.map(({ key, label }) => (
										<FormField
											key={key}
											control={form.control}
											name={key}
											render={({ field }) => (
												<FormItem className="flex items-center justify-between rounded-lg border p-3">
													<FormLabel>{label}</FormLabel>
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
								<dt className="text-sm text-muted-foreground">模型 ID</dt>
								<dd className="min-w-0 truncate font-mono text-sm">{model.providerModelId}</dd>
							</div>
							<div className="grid grid-cols-2 gap-3">
								<div className="rounded-lg border px-4 py-2.5">
									<dt className="text-xs text-muted-foreground">上下文长度</dt>
									<dd className="mt-0.5 text-sm font-medium">
										{model.contextLength.toLocaleString()}
									</dd>
								</div>
								<div className="rounded-lg border px-4 py-2.5">
									<dt className="text-xs text-muted-foreground">最大输出</dt>
									<dd className="mt-0.5 text-sm font-medium">
										{model.maxOutputTokens.toLocaleString()}
									</dd>
								</div>
							</div>
							<div className="rounded-lg border px-4 py-3">
								<dt className="text-xs text-muted-foreground">模型能力</dt>
								<dd className="mt-2 grid grid-cols-2 gap-2">
									{CAPABILITIES.map(({ key, label, icon: Icon }) => (
										<span
											key={key}
											className={
												model[key]
													? "flex items-center gap-1.5 text-sm text-emerald-600 dark:text-emerald-400"
													: "flex items-center gap-1.5 text-sm text-muted-foreground/60"
											}
										>
											<Icon className="size-3.5" />
											{label}
											{model[key] ? "已支持" : "不支持"}
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
									删除
								</Button>
								<Button
									type="submit"
									size="sm"
									form="provider-model-detail-form"
									disabled={updateModel.isPending}
								>
									更新
								</Button>
							</>
						) : (
							<Button type="button" variant="outline" size="sm" onClick={() => setEditing(true)}>
								<Pencil className="mr-1.5 size-4" />
								编辑
							</Button>
						)}
					</DialogFooter>
				</DialogContent>
			</Dialog>

			<ConfirmDialog
				open={confirmingDelete}
				onOpenChange={setConfirmingDelete}
				title="删除供应商模型"
				desc={
					<>
						确定要删除模型 <span className="font-semibold">{model.providerModelId}</span> 吗？
						此操作无法撤销。
					</>
				}
				confirmText="删除"
				destructive
				isLoading={deleteModel.isPending}
				handleConfirm={handleDelete}
			/>
		</>
	);
}
