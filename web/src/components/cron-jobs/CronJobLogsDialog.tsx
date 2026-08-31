import { StatusBadge } from "@/components/status-badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import {
	type CronJobLog,
	type CronJobLogLevel,
	type CronJobRun,
	useCronJobLogStream,
	useCronJobRunLogs,
	useCronJobRuns,
} from "@/hooks/use-cron-job-logs";
import type { CronJob } from "@/hooks/use-cron-jobs";
import { cn } from "@/lib/utils";
import { ArrowDown, ChevronDown, ChevronRight, ScrollText } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface CronJobLogsDialogProps {
	job: CronJob | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 距底部小于该值视为“已在底部”，恢复自动跟随。 */
const SCROLL_BOTTOM_THRESHOLD = 24;

const LEVEL_CLASS: Record<CronJobLogLevel, string> = {
	INFO: "text-sky-500",
	WARN: "text-amber-500",
	ERROR: "text-red-500",
};

function formatDateTime(ts: string) {
	if (!ts) return "—";
	const date = new Date(ts);
	if (Number.isNaN(date.getTime())) return "—";
	const pad = (n: number) => String(n).padStart(2, "0");
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function LogLine({ log }: { log: CronJobLog }) {
	return (
		<div className="flex gap-2 px-3 py-0.5 font-mono text-xs leading-relaxed">
			<span className="shrink-0 whitespace-nowrap text-muted-foreground">
				{formatDateTime(log.ts)}
			</span>
			<span className={cn("w-12 shrink-0", LEVEL_CLASS[log.level] ?? "text-muted-foreground")}>
				{log.level}
			</span>
			<span className="min-w-0 whitespace-pre-wrap break-all">{log.message}</span>
		</div>
	);
}

function runStatusBadge(
	run: CronJobRun,
	t: (key: string, opts?: Record<string, unknown>) => string,
) {
	if (run.status === "running") {
		return <StatusBadge status="warning" label={t("cronJobs.status.running")} />;
	}
	if (run.status === "failed") {
		return <StatusBadge status="error" label={t("cronJobs.status.failed")} />;
	}
	return <StatusBadge status="success" label={t("cronJobs.status.success")} />;
}

function RunItem({
	name,
	run,
	expanded,
	onToggle,
}: {
	name: string;
	run: CronJobRun;
	expanded: boolean;
	onToggle: () => void;
}) {
	const { t } = useTranslation();
	const { data: logs, isLoading } = useCronJobRunLogs(name, expanded ? run.run_id : null);

	return (
		<li>
			<button
				type="button"
				onClick={onToggle}
				className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-slate-100/60 dark:hover:bg-white/5"
			>
				{expanded ? (
					<ChevronDown className="size-4 shrink-0 text-muted-foreground" />
				) : (
					<ChevronRight className="size-4 shrink-0 text-muted-foreground" />
				)}
				{runStatusBadge(run, t)}
				<span className="text-xs text-muted-foreground">
					{formatDateTime(run.started_at)} ~ {run.ended_at ? formatDateTime(run.ended_at) : "—"}
				</span>
				<span className="ml-auto shrink-0 text-xs text-muted-foreground">
					{run.log_count} {t("cronJobs.logCountUnit")}
					{run.truncated && t("cronJobs.truncatedMark")}
				</span>
			</button>
			{expanded && (
				<div className="border-t border-slate-200/70 bg-slate-50/60 py-1 dark:border-white/10 dark:bg-black/20">
					{isLoading ? (
						<p className="px-3 py-1 font-mono text-xs text-muted-foreground">
							{t("common.loading")}
						</p>
					) : logs && logs.length > 0 ? (
						logs.map((log) => <LogLine key={log.seq} log={log} />)
					) : (
						<p className="px-3 py-1 font-mono text-xs text-muted-foreground">
							{t("cronJobs.noOutput")}
						</p>
					)}
				</div>
			)}
		</li>
	);
}

export function CronJobLogsDialog({ job, open, onOpenChange }: CronJobLogsDialogProps) {
	const { t } = useTranslation();
	const name = job?.name ?? "";
	const stream = useCronJobLogStream(open ? name : "");
	const { data: runs } = useCronJobRuns(open ? name : "");

	const [expandedRunId, setExpandedRunId] = useState<string | null>(null);
	const liveRef = useRef<HTMLDivElement>(null);
	const [autoFollow, setAutoFollow] = useState(true);

	// 切换任务时收起历史展开项。
	// biome-ignore lint/correctness/useExhaustiveDependencies: 切换任务（name 变化）即重置展开状态
	useEffect(() => {
		setExpandedRunId(null);
	}, [name]);

	// 处于跟随状态时，新日志到达即滚动到底部。
	// biome-ignore lint/correctness/useExhaustiveDependencies: 日志更新是滚动到底部的触发条件
	useEffect(() => {
		if (autoFollow && liveRef.current) {
			liveRef.current.scrollTop = liveRef.current.scrollHeight;
		}
	}, [stream.logs, autoFollow]);

	const handleScroll = (e: React.UIEvent<HTMLDivElement>) => {
		const el = e.currentTarget;
		const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_BOTTOM_THRESHOLD;
		if (atBottom) {
			// 回到底部立即对齐最新日志，并恢复自动跟随。
			el.scrollTop = el.scrollHeight;
			setAutoFollow(true);
		} else {
			// 用户向上滚动：暂停自动跟随。
			setAutoFollow(false);
		}
	};

	const scrollToLatest = () => {
		const el = liveRef.current;
		if (el) {
			el.scrollTop = el.scrollHeight;
		}
		setAutoFollow(true);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-0 p-0">
				<DialogHeader className="px-6 pb-4 pt-6">
					<DialogTitle className="flex items-center gap-2">
						<ScrollText className="size-5" />
						{t("cronJobs.logs")} · {job?.name}
					</DialogTitle>
				</DialogHeader>

				<div className="flex min-h-0 flex-1 flex-col gap-4 px-6 pb-6">
					{/* 实时日志区 */}
					<div className="relative flex h-64 shrink-0 flex-col overflow-hidden rounded-xl border border-white/70 bg-white/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04]">
						<div className="flex items-center justify-between border-b border-slate-200/70 bg-white/50 px-3 py-2 dark:border-white/10 dark:bg-white/5">
							<div className="flex items-center gap-2 text-sm font-medium">
								{t("cronJobs.realTimeLogs")}
								{stream.currentRun && (
									<span className="text-xs text-muted-foreground">
										{t("cronJobs.startedAt")} {formatDateTime(stream.currentRun.started_at)}
										{!stream.ended && t("cronJobs.runningEllipsis")}
									</span>
								)}
							</div>
							{stream.connection === "reconnecting" && (
								<span className="text-xs text-amber-500">{t("cronJobs.reconnecting")}</span>
							)}
						</div>
						<div
							ref={liveRef}
							onScroll={handleScroll}
							className="min-h-0 flex-1 overflow-y-auto bg-white/30 py-1 dark:bg-black/20"
						>
							{!stream.currentRun ? (
								<div className="flex h-full items-center justify-center text-sm text-muted-foreground">
									{t("cronJobs.noActiveRun")}
								</div>
							) : (
								<>
									{stream.logs.map((log) => (
										<LogLine key={log.seq} log={log} />
									))}
									{stream.ended && (
										<div className="mt-1 border-t px-3 py-2 text-xs text-muted-foreground">
											{t("cronJobs.runEnded")}
											{stream.ended.status === "success"
												? t("cronJobs.status.success")
												: t("cronJobs.status.failed")}{" "}
											· {t("cronJobs.startedAt")}{" "}
											{stream.currentRun ? formatDateTime(stream.currentRun.started_at) : "—"} ~{" "}
											{t("cronJobs.endedAtLabel")} {formatDateTime(stream.ended.ended_at)}
											{stream.ended.truncated && t("cronJobs.truncatedAtLimit")}
										</div>
									)}
								</>
							)}
						</div>
						{!autoFollow && stream.logs.length > 0 && (
							<Button
								variant="secondary"
								size="sm"
								className="absolute bottom-3 right-3 shadow-md"
								onClick={scrollToLatest}
							>
								<ArrowDown className="mr-1 size-3.5" />
								{t("cronJobs.backToLatest")}
							</Button>
						)}
					</div>

					{/* 历史执行区 */}
					<div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-white/70 bg-white/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-xl dark:border-white/10 dark:bg-white/[0.04]">
						<div className="border-b border-slate-200/70 bg-white/50 px-3 py-2 text-sm font-medium dark:border-white/10 dark:bg-white/5">
							{t("cronJobs.historyRuns")}
						</div>
						<div className="min-h-0 flex-1 overflow-y-auto">
							{!runs || runs.length === 0 ? (
								<div className="flex h-full items-center justify-center text-sm text-muted-foreground">
									{t("cronJobs.noRunLogs")}
								</div>
							) : (
								<ul className="divide-y divide-slate-200/70 dark:divide-white/10">
									{runs.map((run) => (
										<RunItem
											key={run.run_id}
											name={name}
											run={run}
											expanded={expandedRunId === run.run_id}
											onToggle={() =>
												setExpandedRunId(expandedRunId === run.run_id ? null : run.run_id)
											}
										/>
									))}
								</ul>
							)}
						</div>
					</div>
				</div>
			</DialogContent>
		</Dialog>
	);
}
