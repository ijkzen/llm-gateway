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
import { type CronJob, useUpdateCronJob } from "@/hooks/use-cron-jobs";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

const cronJobFormSchema = z.object({
	title: z.string().min(1, "标题不能为空"),
	description: z.string(),
	expression: z.string().min(1, "Cron 表达式不能为空"),
	group: z.string(),
});

type CronJobFormValues = z.infer<typeof cronJobFormSchema>;

interface CronJobEditDialogProps {
	job: CronJob | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function CronJobEditDialog({ job, open, onOpenChange }: CronJobEditDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const updateCronJob = useUpdateCronJob();

	const form = useForm<CronJobFormValues>({
		resolver: zodResolver(cronJobFormSchema),
		defaultValues: {
			title: "",
			description: "",
			expression: "",
			group: "",
		},
	});

	useEffect(() => {
		if (job) {
			form.reset({
				title: job.title,
				description: job.description,
				expression: job.expression,
				group: job.group,
			});
		}
	}, [job, form]);

	useEffect(() => {
		if (!open) {
			form.reset();
		}
	}, [open, form]);

	const onSubmit = (values: CronJobFormValues) => {
		if (!job) return;
		updateCronJob.mutate(
			{ name: job.name, ...values },
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
					<DialogTitle>编辑任务</DialogTitle>
					<DialogDescription>修改定时任务的基本信息</DialogDescription>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)}>
						<div className="grid gap-4 py-4">
							<FormField
								control={form.control}
								name="title"
								render={({ field }) => (
									<FormItem>
										<FormLabel>标题</FormLabel>
										<FormControl>
											<Input {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<FormField
								control={form.control}
								name="group"
								render={({ field }) => (
									<FormItem>
										<FormLabel>分组</FormLabel>
										<FormControl>
											<Input {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<FormField
								control={form.control}
								name="description"
								render={({ field }) => (
									<FormItem>
										<FormLabel>描述</FormLabel>
										<FormControl>
											<Input {...field} />
										</FormControl>
										<FormMessage />
									</FormItem>
								)}
							/>
							<FormField
								control={form.control}
								name="expression"
								render={({ field }) => (
									<FormItem>
										<FormLabel>Cron 表达式</FormLabel>
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
							<Button type="submit" disabled={updateCronJob.isPending}>
								保存
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
