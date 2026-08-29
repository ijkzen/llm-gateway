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
import { useCreateApiKey } from "@/hooks/use-api-keys";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const formSchema = z.object({
	name: z.string().min(1, "名称不能为空"),
});

type FormValues = z.infer<typeof formSchema>;

interface ApiKeyCreateDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 创建 API Key：仅需填写名称，密钥由服务端自动生成。 */
export function ApiKeyCreateDialog({ open, onOpenChange }: ApiKeyCreateDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const createApiKey = useCreateApiKey();

	const form = useForm<FormValues>({
		resolver: zodResolver(formSchema),
		defaultValues: { name: "" },
	});

	// 打开弹窗时重置表单。
	useEffect(() => {
		if (!open) return;
		form.reset({ name: "" });
	}, [open, form]);

	const onSubmit = (values: FormValues) => {
		createApiKey.mutate(
			{ name: values.name.trim() },
			{
				onSuccess: () => {
					onOpenChange(false);
					toastSuccess("创建成功");
				},
				onError: (error) => toastError("创建失败", error),
			},
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-[480px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>创建 API Key</DialogTitle>
					<DialogDescription>
						填写名称后系统将自动生成密钥，可随时在列表中点击小眼睛查看明文。
					</DialogDescription>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
						<FormField
							control={form.control}
							name="name"
							render={({ field }) => (
								<FormItem>
									<FormLabel required>名称</FormLabel>
									<FormControl>
										<Input placeholder="如 my-laptop" autoComplete="off" {...field} />
									</FormControl>
									<FormMessage />
								</FormItem>
							)}
						/>
						<DialogFooter className="gap-2 pt-2">
							<Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
								取消
							</Button>
							<Button type="submit" disabled={createApiKey.isPending}>
								创建
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
