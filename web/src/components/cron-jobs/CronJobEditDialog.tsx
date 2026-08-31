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
import { type CronJob, useUpdateCronJob } from "@/hooks/use-cron-jobs";
import { useToastActions } from "@/hooks/use-toast";
import { zodResolver } from "@hookform/resolvers/zod";
import { useEffect, useMemo } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

function makeSchema(t: (key: string) => string) {
	return z.object({
		title: z.string().min(1, t("cronJobs.emptyTitle")),
		description: z.string(),
		expression: z.string().min(1, t("cronJobs.emptyCronExpression")),
		group: z.string(),
	});
}

type CronJobFormValues = z.infer<ReturnType<typeof makeSchema>>;

interface CronJobEditDialogProps {
	job: CronJob | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function CronJobEditDialog({ job, open, onOpenChange }: CronJobEditDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess, toastError } = useToastActions();
	const updateCronJob = useUpdateCronJob();
	const cronJobFormSchema = useMemo(() => makeSchema(t), [t]);

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
					toastSuccess(t("common.updateSuccess"));
				},
				onError: (error) => {
					toastError(t("common.updateFailed"), error);
				},
			},
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-[500px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>{t("cronJobs.editTitle")}</DialogTitle>
				</DialogHeader>
				<Form {...form}>
					<form onSubmit={form.handleSubmit(onSubmit)}>
						<div className="grid gap-4 py-4">
							<FormField
								control={form.control}
								name="title"
								render={({ field }) => (
									<FormItem>
										<FormLabel>{t("cronJobs.titleLabel")}</FormLabel>
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
										<FormLabel>{t("cronJobs.group")}</FormLabel>
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
										<FormLabel>{t("cronJobs.description")}</FormLabel>
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
										<FormLabel>{t("cronJobs.cronExpression")}</FormLabel>
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
								{t("common.cancel")}
							</Button>
							<Button type="submit" disabled={updateCronJob.isPending}>
								{t("common.save")}
							</Button>
						</DialogFooter>
					</form>
				</Form>
			</DialogContent>
		</Dialog>
	);
}
