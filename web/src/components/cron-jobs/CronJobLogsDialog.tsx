import { MidEllipsis } from "@/components/mid-ellipsis";
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
import { ArrowDown, ScrollText } from "lucide-react";
import { memo, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface CronJobLogsDialogProps {
	job: CronJob | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 距底部小于该值视为“已在底部”，恢复自动跟随。 */
const SCROLL_BOTTOM_THRESHOLD = 24;

const LEVEL_CLASS: Record<CronJobLogLevel, string> = {
	INFO: "text-info",
	WARN: "text-warning",
	ERROR: "text-destructive",
};

function formatDateTime(ts: string) {
	if (!ts) return "—";
	const date = new Date(ts);
	if (Number.isNaN(date.getTime())) return "—";
	const pad = (n: number) => String(n).padStart(2, "0");
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function formatRunRange(run: CronJobRun) {
	return `${formatDateTime(run.started_at)} ~ ${run.ended_at ? formatDateTime(run.ended_at) : "—"}`;
}

/** 单行日志（18-09：memo + 时间戳随日志对象预格式化，追加日志时旧行不重算不重渲）。 */
const LogLine = memo(function LogLine({ log }: { log: CronJobLog }) {
	return (
		<div className="px-3 py-0.5 font-mono text-xs leading-relaxed">
			<div className="flex gap-2">
				<span className="shrink-0 whitespace-nowrap text-muted-foreground">
					{formatDateTime(log.ts)}
				</span>
				<span className={cn("w-12 shrink-0", LEVEL_CLASS[log.level] ?? "text-muted-foreground")}>
					{log.level}
				</span>
			</div>
			<div className="whitespace-pre-wrap break-all">{log.message}</div>
		</div>
	);
});

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
	run,
	selected,
	onSelect,
}: {
	run: CronJobRun;
	selected: boolean;
	onSelect: () => void;
}) {
	const { t } = useTranslation();

	return (
		<li>
			<button
				type="button"
				onClick={onSelect}
				aria-pressed={selected}
				className={cn(
					"flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-foreground/5",
					selected && "bg-foreground/5",
				)}
			>
				{runStatusBadge(run, t)}
				<span className="text-xs text-muted-foreground">{formatRunRange(run)}</span>
				<span className="ml-auto shrink-0 text-xs text-muted-foreground">
					{run.log_count} {t("cronJobs.logCountUnit")}
					{run.truncated && t("cronJobs.truncatedMark")}
				</span>
			</button>
		</li>
	);
}

export function CronJobLogsDialog({ job, open, onOpenChange }: CronJobLogsDialogProps) {
	const { t } = useTranslation();
	const name = job?.name ?? "";
	const stream = useCronJobLogStream(open ? name : "");
	const { data: runs } = useCronJobRuns(open ? name : "");

	const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
	const { data: historyLogs, isLoading: historyLoading } = useCronJobRunLogs(
		name,
		open ? selectedRunId : null,
	);
	const liveRef = useRef<HTMLDivElement>(null);
	const [autoFollow, setAutoFollow] = useState(true);

	const selectedRun = runs?.find((run) => run.run_id === selectedRunId) ?? null;

	// 切换任务或关闭弹窗即回到实时模式。
	// biome-ignore lint/correctness/useExhaustiveDependencies: 只需在 name/open 变化时重置选中项
	useEffect(() => {
		setSelectedRunId(null);
	}, [name, open]);

	// 处于跟随状态时，新日志到达或从历史模式切回实时即滚动到底部。
	// biome-ignore lint/correctness/useExhaustiveDependencies: 日志更新与模式切换是滚动到底部的触发条件
	useEffect(() => {
		if (autoFollow && liveRef.current) {
			liveRef.current.scrollTop = liveRef.current.scrollHeight;
		}
	}, [stream.logs, autoFollow, selectedRunId]);

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
					{/* 日志区：实时（默认）或所选历史执行 */}
					<div className="relative flex h-80 shrink-0 flex-col overflow-hidden rounded-lg border border-border bg-background">
						<div className="flex items-center justify-between gap-2 border-b border-border bg-muted px-3 py-2">
							{selectedRun ? (
								<div className="flex min-w-0 items-center gap-2 text-sm font-medium">
									<span className="shrink-0">{t("cronJobs.historyLogs")}</span>
									<span className="shrink-0 text-muted-foreground">·</span>
									<MidEllipsis
										text={formatRunRange(selectedRun)}
										className="text-xs font-normal text-muted-foreground"
									/>
								</div>
							) : (
								<div className="flex items-center gap-2 text-sm font-medium">
									{t("cronJobs.realTimeLogs")}
									{stream.currentRun && (
										<span className="text-xs text-muted-foreground">
											{t("cronJobs.startedAt")} {formatDateTime(stream.currentRun.started_at)}
											{!stream.ended && t("cronJobs.runningEllipsis")}
										</span>
									)}
								</div>
							)}
							{selectedRun ? (
								<Button
									type="button"
									variant="outline"
									size="sm"
									className="h-6 shrink-0 px-2 text-xs"
									onClick={() => setSelectedRunId(null)}
								>
									{t("cronJobs.backToLive")}
								</Button>
							) : (
								<>
									{stream.connection === "reconnecting" && (
										<span className="text-xs text-warning">{t("cronJobs.reconnecting")}</span>
									)}
									{/* 18-11：退避重连达上限后停止静默转圈，提示用户手动刷新（会话过期等）。 */}
									{stream.reconnectExhausted && (
										<span className="flex items-center gap-2 text-xs text-destructive">
											{t("cronJobs.reconnectFailed")}
											<Button
												type="button"
												variant="outline"
												size="sm"
												className="h-6 px-2 text-xs"
												onClick={() => window.location.reload()}
											>
												{t("common.refresh")}
											</Button>
										</span>
									)}
								</>
							)}
						</div>
						<div
							ref={selectedRun ? undefined : liveRef}
							onScroll={selectedRun ? undefined : handleScroll}
							className="min-h-0 flex-1 overflow-y-auto bg-muted/30 py-1 dark:bg-black/20"
						>
							{selectedRun ? (
								historyLoading ? (
									<p className="px-3 py-1 font-mono text-xs text-muted-foreground">
										{t("common.loading")}
									</p>
								) : historyLogs && historyLogs.length > 0 ? (
									historyLogs.map((log) => <LogLine key={log.seq} log={log} />)
								) : (
									<p className="px-3 py-1 font-mono text-xs text-muted-foreground">
										{t("cronJobs.noOutput")}
									</p>
								)
							) : !stream.currentRun ? (
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
						{!selectedRun && !autoFollow && stream.logs.length > 0 && (
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
					<div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border bg-background">
						<div className="border-b border-border bg-muted px-3 py-2 text-sm font-medium">
							{t("cronJobs.historyRuns")}
						</div>
						<div className="min-h-0 flex-1 overflow-y-auto">
							{!runs || runs.length === 0 ? (
								<div className="flex h-full items-center justify-center text-sm text-muted-foreground">
									{t("cronJobs.noRunLogs")}
								</div>
							) : (
								<ul className="divide-y divide-border/70">
									{runs.map((run) => (
										<RunItem
											key={run.run_id}
											run={run}
											selected={selectedRunId === run.run_id}
											onSelect={() =>
												setSelectedRunId(selectedRunId === run.run_id ? null : run.run_id)
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
