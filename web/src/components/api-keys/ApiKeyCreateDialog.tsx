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
import { useEffect, useMemo } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

function makeSchema(t: (key: string) => string) {
	return z.object({
		name: z.string().min(1, t("apiKeys.nameRequired")),
	});
}

type FormValues = z.infer<ReturnType<typeof makeSchema>>;

interface ApiKeyCreateDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 创建 API Key：仅需填写名称，密钥由服务端自动生成。 */
export function ApiKeyCreateDialog({ open, onOpenChange }: ApiKeyCreateDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const createApiKey = useCreateApiKey();
	const formSchema = useMemo(() => makeSchema(t), [t]);

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
					toastSuccess(t("common.createSuccess"));
				},
				onError: (error) => toastError(t("common.createFailed"), error),
			},
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-[480px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>{t("apiKeys.createDialogTitle")}</DialogTitle>
					<DialogDescription>{t("apiKeys.createDialogDesc")}</DialogDescription>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
						<FormField
							control={form.control}
							name="name"
							render={({ field }) => (
								<FormItem>
									<FormLabel required>{t("apiKeys.name")}</FormLabel>
									<FormControl>
										<Input
											placeholder={t("apiKeys.placeholderName")}
											autoComplete="off"
											{...field}
										/>
									</FormControl>
									<FormMessage />
								</FormItem>
							)}
						/>
						<DialogFooter className="gap-2 pt-2">
							<Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
								{t("common.cancel")}
							</Button>
							<Button type="submit" disabled={createApiKey.isPending}>
								{t("common.create")}
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
