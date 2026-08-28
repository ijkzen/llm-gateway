import { StatusBadge } from "@/components/status-badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { CronJob } from "@/hooks/use-cron-jobs";
import { DEFAULT_GROUP } from "@/lib/constants";
import { cn } from "@/lib/utils";
import { useMemo } from "react";

interface CronJobListProps {
	jobs: CronJob[] | undefined;
	selectedName: string | null;
	onSelect: (job: CronJob) => void;
}

export function CronJobList({ jobs, selectedName, onSelect }: CronJobListProps) {
	const groupedJobs = useMemo(() => {
		const map = new Map<string, CronJob[]>();
		for (const job of jobs ?? []) {
			const key = job.group || DEFAULT_GROUP;
			const list = map.get(key) ?? [];
			list.push(job);
			map.set(key, list);
		}
		return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
	}, [jobs]);

	if (!jobs || jobs.length === 0) {
		return (
			<Card>
				<CardContent className="p-8 text-center text-muted-foreground">暂无定时任务</CardContent>
			</Card>
		);
	}

	return (
		<div className="space-y-4">
			{groupedJobs.map(([group, groupJobs]) => (
				<Card key={group}>
					<CardHeader className="py-4">
						<CardTitle className="text-sm font-medium text-muted-foreground">{group}</CardTitle>
					</CardHeader>
					<CardContent className="p-0">
						<ul className="divide-y">
							{groupJobs.map((job) => (
								<li key={job.name}>
									<button
										type="button"
										onClick={() => onSelect(job)}
										className={cn(
											"flex w-full items-center justify-between gap-3 px-4 py-3 text-left transition-colors",
											selectedName === job.name
												? "bg-foreground text-background dark:bg-primary dark:text-primary-foreground"
												: "hover:bg-slate-100/60 dark:hover:bg-white/5",
										)}
									>
										<div className="min-w-0">
											<p className="truncate font-medium">{job.name}</p>
											<p
												className={cn(
													"truncate text-xs",
													selectedName === job.name
														? "text-background/60 dark:text-primary-foreground/60"
														: "text-muted-foreground",
												)}
											>
												{job.title}
											</p>
										</div>
										<StatusBadge status={job.enabled ? "enabled" : "disabled"} />
									</button>
								</li>
							))}
						</ul>
					</CardContent>
				</Card>
			))}
		</div>
	);
}
