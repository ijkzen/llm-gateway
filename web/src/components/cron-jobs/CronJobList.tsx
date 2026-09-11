import { MidEllipsis } from "@/components/mid-ellipsis";
import { StatusBadge } from "@/components/status-badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { CronJob } from "@/hooks/use-cron-jobs";
import { DEFAULT_GROUP } from "@/lib/constants";
import { cn } from "@/lib/utils";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

interface CronJobListProps {
	jobs: CronJob[] | undefined;
	selectedName: string | null;
	onSelect: (job: CronJob) => void;
}

export function CronJobList({ jobs, selectedName, onSelect }: CronJobListProps) {
	const { t } = useTranslation();
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
				<CardContent className="p-8 text-center text-muted-foreground">
					{t("cronJobs.noJobs")}
				</CardContent>
			</Card>
		);
	}

	return (
		<div className="space-y-4">
			{groupedJobs.map(([group, groupJobs]) => (
				<Card key={group}>
					<CardHeader className="py-4">
						<CardTitle className="text-sm font-medium text-muted-foreground">
							{/* 20-05：未分组任务在英文界面显示 Default，存储值仍是后端种子值。 */}
							{group === DEFAULT_GROUP ? t("cronJobs.defaultGroup") : group}
						</CardTitle>
					</CardHeader>
					<CardContent className="p-2">
						<ul className="space-y-1">
							{groupJobs.map((job) => (
								<li key={job.name}>
									<button
										type="button"
										onClick={() => onSelect(job)}
										className={cn(
											"flex w-full items-center justify-between gap-3 rounded-md px-4 py-3 text-left transition-colors",
											selectedName === job.name
												? "bg-sidebar-accent text-sidebar-accent-foreground"
												: "hover:bg-foreground/5",
										)}
									>
										<div className="min-w-0">
											<MidEllipsis text={job.name} className="font-medium" />
											<MidEllipsis
												className={cn(
													"text-xs",
													selectedName === job.name
														? "text-sidebar-accent-foreground/60"
														: "text-muted-foreground",
												)}
												text={job.title}
											/>
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
