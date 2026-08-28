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
import { type Setting, useUpdateSetting } from "@/hooks/use-settings";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const settingFormSchema = z.object({
	value: z.string(),
});

type SettingFormValues = z.infer<typeof settingFormSchema>;

interface SettingEditDialogProps {
	setting: Setting | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function SettingEditDialog({ setting, open, onOpenChange }: SettingEditDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const updateSetting = useUpdateSetting();

	const form = useForm<SettingFormValues>({
		resolver: zodResolver(settingFormSchema),
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
								render={({ field }) => (
									<FormItem>
										<FormLabel>值</FormLabel>
										<FormControl>
											<Input {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
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
