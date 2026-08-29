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
import { useChangePassword } from "@/hooks/use-auth";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const changePasswordSchema = z
	.object({
		oldPassword: z.string().min(1, "请输入旧密码"),
		newPassword: z.string().min(6, "新密码至少 6 个字符").max(128, "新密码最多 128 个字符"),
		confirmPassword: z.string(),
	})
	.refine((values) => values.newPassword === values.confirmPassword, {
		message: "两次输入的新密码不一致",
		path: ["confirmPassword"],
	});

type ChangePasswordValues = z.infer<typeof changePasswordSchema>;

interface ChangePasswordDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function ChangePasswordDialog({ open, onOpenChange }: ChangePasswordDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const changePassword = useChangePassword();

	const form = useForm<ChangePasswordValues>({
		resolver: zodResolver(changePasswordSchema),
		defaultValues: { oldPassword: "", newPassword: "", confirmPassword: "" },
	});

	useEffect(() => {
		if (!open) {
			form.reset();
		}
	}, [open, form]);

	const onSubmit = (values: ChangePasswordValues) => {
		changePassword.mutate(
			{ oldPassword: values.oldPassword, newPassword: values.newPassword },
			{
				onSuccess: () => {
					onOpenChange(false);
					toastSuccess("密码修改成功");
				},
				onError: (error) => {
					toastError("修改密码失败", error);
				},
			},
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-[440px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>修改密码</DialogTitle>
					<DialogDescription>修改成功后，其他已登录的会话将被强制下线。</DialogDescription>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)}>
						<div className="grid gap-4 py-2">
							<FormField
								control={form.control}
								name="oldPassword"
								render={({ field }) => (
									<FormItem>
										<FormLabel>旧密码</FormLabel>
										<FormControl>
											<Input type="password" autoComplete="current-password" {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<FormField
								control={form.control}
								name="newPassword"
								render={({ field }) => (
									<FormItem>
										<FormLabel>新密码</FormLabel>
										<FormControl>
											<Input type="password" autoComplete="new-password" {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<FormField
								control={form.control}
								name="confirmPassword"
								render={({ field }) => (
									<FormItem>
										<FormLabel>确认新密码</FormLabel>
										<FormControl>
											<Input type="password" autoComplete="new-password" {...field} />
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
							<Button type="submit" disabled={changePassword.isPending}>
								确认修改
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
