import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";

export type CronJobRunStatus = "running" | "success" | "failed";

export type CronJobLogLevel = "INFO" | "WARN" | "ERROR";

/** 一次执行的元信息（对应 GET /api/cron-jobs/{name}/logs） */
export interface CronJobRun {
	run_id: string;
	job_name: string;
	status: CronJobRunStatus;
	started_at: string;
	ended_at: string;
	log_count: number;
	truncated: boolean;
}

/** 一条日志（对应 GET /api/cron-jobs/{name}/logs/{run_id} 与 SSE 事件） */
export interface CronJobLog {
	seq: number;
	level: CronJobLogLevel;
	message: string;
	ts: string;
}

/** SSE 实时流的当前状态 */
export interface CronJobLogStreamState {
	/** 连接状态：open / reconnecting / closed */
	connection: "open" | "reconnecting" | "closed";
	/** 当前正在执行的 run（无执行时为空） */
	currentRun: { run_id: string; started_at: string } | null;
	/** 当前 run 已接收的日志（按 seq 升序，去重） */
	logs: CronJobLog[];
	/** 当前 run 已结束时的状态（无执行中或未结束时为空） */
	ended: { status: CronJobRunStatus; ended_at: string; truncated: boolean } | null;
}

const initialStreamState: CronJobLogStreamState = {
	connection: "closed",
	currentRun: null,
	logs: [],
	ended: null,
};

export const cronJobLogsKeys = {
	runs: (name: string) => ["cron-job-logs", name] as const,
	runLogs: (name: string, runId: string) => ["cron-job-run-logs", name, runId] as const,
};

async function fetchRunLogs(name: string, runId: string): Promise<CronJobLog[]> {
	const res = await api.get(`cron-jobs/${name}/logs/${runId}`).json<ApiResponse<CronJobLog[]>>();
	return unwrap(res);
}

/** 最近 30 次执行的列表 */
export function useCronJobRuns(name: string) {
	return useQuery<CronJobRun[]>({
		queryKey: cronJobLogsKeys.runs(name),
		queryFn: async () => {
			const res = await api.get(`cron-jobs/${name}/logs`).json<ApiResponse<CronJobRun[]>>();
			return unwrap(res);
		},
		enabled: !!name,
	});
}

/** 某次执行的日志（懒加载） */
export function useCronJobRunLogs(name: string, runId: string | null) {
	return useQuery<CronJobLog[]>({
		queryKey: cronJobLogsKeys.runLogs(name, runId ?? ""),
		queryFn: () => fetchRunLogs(name, runId ?? ""),
		enabled: !!name && !!runId,
	});
}

/**
 * 订阅任务日志 SSE 实时流。
 *
 * 事件契约（后端 /api/cron-jobs/{name}/logs/stream）：
 * - `snapshot`：连接建立时存在执行中的 run，回放其已落库日志
 * - `idle`：连接建立时没有执行中的 run
 * - `log`：执行中的一条新日志（携带 run_id，按 seq 去重）
 * - `run_started` / `run_ended`：执行生命周期事件
 * - `reset`：接收端积压丢事件，需重新拉取当前 run 全量日志
 */
export function useCronJobLogStream(name: string) {
	const queryClient = useQueryClient();
	const [state, setState] = useState<CronJobLogStreamState>(initialStreamState);
	const stateRef = useRef(state);
	stateRef.current = state;

	useEffect(() => {
		if (!name) {
			setState(initialStreamState);
			return;
		}

		const es = new EventSource(`/api/cron-jobs/${encodeURIComponent(name)}/logs/stream`);

		es.onopen = () => setState((s) => ({ ...s, connection: "open" }));
		es.onerror = () => setState((s) => ({ ...s, connection: "reconnecting" }));

		es.addEventListener("snapshot", (e: MessageEvent) => {
			const data = JSON.parse(e.data) as {
				run_id: string;
				started_at: string;
				logs: CronJobLog[];
			};
			setState({
				connection: "open",
				currentRun: { run_id: data.run_id, started_at: data.started_at },
				logs: data.logs,
				ended: null,
			});
		});

		es.addEventListener("idle", () => {
			setState((s) => ({ ...s, currentRun: null, logs: [], ended: null }));
		});

		es.addEventListener("log", (e: MessageEvent) => {
			const data = JSON.parse(e.data) as {
				run_id: string;
				seq: number;
				level: CronJobLogLevel;
				message: string;
				ts: string;
			};
			setState((s) => {
				if (s.currentRun?.run_id !== data.run_id) return s;
				const last = s.logs[s.logs.length - 1];
				// 与 snapshot 回放可能重叠，按 seq 去重。
				if (last && data.seq <= last.seq) return s;
				return {
					...s,
					logs: [
						...s.logs,
						{ seq: data.seq, level: data.level, message: data.message, ts: data.ts },
					],
				};
			});
		});

		es.addEventListener("run_started", (e: MessageEvent) => {
			const data = JSON.parse(e.data) as { run_id: string; ts: string };
			setState((s) => ({
				...s,
				currentRun: { run_id: data.run_id, started_at: data.ts },
				logs: [],
				ended: null,
			}));
			queryClient.invalidateQueries({ queryKey: cronJobLogsKeys.runs(name) });
		});

		es.addEventListener("run_ended", (e: MessageEvent) => {
			const data = JSON.parse(e.data) as {
				run_id: string;
				status: CronJobRunStatus;
				truncated: boolean;
				ts: string;
			};
			setState((s) => {
				if (s.currentRun?.run_id !== data.run_id) return s;
				return {
					...s,
					ended: { status: data.status, ended_at: data.ts, truncated: data.truncated },
				};
			});
			queryClient.invalidateQueries({ queryKey: cronJobLogsKeys.runs(name) });
		});

		es.addEventListener("reset", () => {
			// 接收端积压丢事件：重新拉取当前 run 的全量日志替换本地。
			const current = stateRef.current;
			if (!current.currentRun) return;
			const runId = current.currentRun.run_id;
			queryClient
				.fetchQuery({
					queryKey: cronJobLogsKeys.runLogs(name, runId),
					queryFn: () => fetchRunLogs(name, runId),
				})
				.then((logs) => {
					setState((s) => (s.currentRun?.run_id === runId ? { ...s, logs } : s));
				})
				.catch(() => {
					// 拉取失败保持现状，后续增量按 seq 去重仍会补齐
				});
		});

		return () => {
			es.close();
		};
	}, [name, queryClient]);

	return state;
}
