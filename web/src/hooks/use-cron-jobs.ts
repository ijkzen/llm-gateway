import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

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

export function useRunCronJob() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (name: string) => {
			const res = await api.post(`cron-jobs/${name}/run`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => {
			// 任务在后端异步执行，延迟刷新才能拿到执行后的 last_run_at
			setTimeout(() => {
				queryClient.invalidateQueries({ queryKey: cronJobsKeys.all });
			}, 1000);
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
