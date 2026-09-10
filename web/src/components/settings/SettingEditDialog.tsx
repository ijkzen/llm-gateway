import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
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
import { type Setting, useUpdateSetting } from "@/hooks/use-settings";
import { useToastActions } from "@/hooks/use-toast";
import type { SettingType } from "@/lib/constants";
import { zodResolver } from "@hookform/resolvers/zod";
import { useEffect, useMemo } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

/**
 * 按设置声明类型校验（18-04）：与后端 validate_setting_value 同口径，避免
 * Int/Bool/Float 填非法值要提交后才吃 400。String/Json 走原文本（Json 另有
 * 结构化编辑弹窗）。
 */
function buildSettingSchema(type: SettingType | undefined, t: (key: string) => string) {
	return z.object({
		value: z
			.string()
			.refine((v) => type !== "Int" || /^-?\d+$/.test(v.trim()), {
				message: t("settings.validationInt"),
			})
			.refine((v) => type !== "Float" || Number.isFinite(Number(v.trim())), {
				message: t("settings.validationFloat"),
			})
			.refine((v) => type !== "Bool" || v.trim() === "true" || v.trim() === "false", {
				message: t("settings.validationBool"),
			})
			.transform((v) => (type === "Bool" ? v.trim() : v)),
	});
}

type SettingFormValues = z.infer<ReturnType<typeof buildSettingSchema>>;

interface SettingEditDialogProps {
	setting: Setting | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function SettingEditDialog({ setting, open, onOpenChange }: SettingEditDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const updateSetting = useUpdateSetting();
	const { t } = useTranslation();
	// 18-04：按声明类型校验（Int/Bool/Float 不再等到提交后吃后端 400）。
	const schema = useMemo(
		() => buildSettingSchema(setting?.type as SettingType | undefined, t),
		[setting?.type, t],
	);

	const form = useForm<SettingFormValues>({
		resolver: zodResolver(schema),
		defaultValues: {
			value: "",
		},
	});

	useEffect(() => {
		if (setting) {
			form.reset({
				value: setting.value,
			});
		}
	}, [setting, form]);

	useEffect(() => {
		if (!open) {
			form.reset();
		}
	}, [open, form]);

	const onSubmit = (values: SettingFormValues) => {
		if (!setting) return;
		updateSetting.mutate(
			{ key: setting.key, value: values.value },
			{
				onSuccess: () => {
					onOpenChange(false);
					toastSuccess("更新成功");
				},
				onError: (error) => {
					toastError("更新失败", error);
				},
			},
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-[500px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>编辑设置</DialogTitle>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)}>
						<div className="grid gap-4 py-4">
							<FormField
								control={form.control}
								name="value"
								render={({ field }) =>
									setting?.type === "Bool" ? (
										<FormItem className="flex items-center justify-between rounded-lg border p-3">
											<FormLabel>{t("settings.value")}</FormLabel>
											<FormControl>
												<Switch
													checked={field.value === "true"}
													onCheckedChange={(checked) => field.onChange(checked ? "true" : "false")}
												/>
											</FormControl>
											<FormMessage />
										</FormItem>
									) : (
										<FormItem>
											<FormLabel>{t("settings.value")}</FormLabel>
											<FormControl>
												<Input
													{...field}
													inputMode={
														setting?.type === "Int"
															? "numeric"
															: setting?.type === "Float"
																? "decimal"
																: undefined
													}
												/>
											</FormControl>
											<FormMessage />
										</FormItem>
									)
								}
							/>
						</div>
						<DialogFooter className="gap-2">
							<Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
								取消
							</Button>
							<Button type="submit" disabled={updateSetting.isPending}>
								保存
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
