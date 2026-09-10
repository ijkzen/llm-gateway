import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";

export interface CronJob {
	name: string;
	title: string;
	description: string;
	expression: string;
	enabled: boolean;
	group: string;
	last_run_at: string;
	next_run_at: string;
	updated_at: string;
	frequency_secs: number;
}

export const cronJobsKeys = {
	all: ["cron-jobs"] as const,
};

export function useCronJobs() {
	return useQuery<CronJob[]>({
		queryKey: cronJobsKeys.all,
		queryFn: async () => {
			const res = await api.get("cron-jobs").json<ApiResponse<CronJob[]>>();
			return unwrap(res);
		},
	});
}

export function useUpdateCronJob() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: { name: string } & Partial<CronJob>) => {
			const { name, ...body } = payload;
			const res = await api.put(`cron-jobs/${name}`, { json: body }).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: cronJobsKeys.all }),
	});
}

/** 手动触发后的刷新节奏（18-07）：last_run_at 在执行**结束**才回写，长任务
 *  固定 1 秒刷新拿不到新值；改为轮询到该字段变化或达上限。 */
const RUN_REFRESH_INTERVAL_MS = 2_000;
const RUN_REFRESH_MAX_TRIES = 15;

export function useRunCronJob() {
	const queryClient = useQueryClient();
	const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	useEffect(
		() => () => {
			if (timerRef.current) clearTimeout(timerRef.current);
		},
		[],
	);

	return useMutation({
		mutationFn: async (name: string) => {
			const res = await api.post(`cron-jobs/${name}/run`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: (_data, name) => {
			const lastRunAt = () =>
				queryClient.getQueryData<CronJob[]>(cronJobsKeys.all)?.find((j) => j.name === name)
					?.last_run_at;
			const before = lastRunAt();
			queryClient.invalidateQueries({ queryKey: cronJobsKeys.all });

			let tries = 0;
			const poll = () => {
				tries += 1;
				// 值已更新（或任务已从列表消失）：停止轮询。
				const now = lastRunAt();
				if ((now !== undefined && now !== before) || tries > RUN_REFRESH_MAX_TRIES) return;
				queryClient.invalidateQueries({ queryKey: cronJobsKeys.all });
				timerRef.current = setTimeout(poll, RUN_REFRESH_INTERVAL_MS);
			};
			timerRef.current = setTimeout(poll, RUN_REFRESH_INTERVAL_MS);
		},
	});
}

export function useDeleteCronJob() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (name: string) => {
			const res = await api.delete(`cron-jobs/${name}`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: cronJobsKeys.all }),
	});
}
