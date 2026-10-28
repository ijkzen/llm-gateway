import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

/** 一次测速运行的状态（四态，与后端 speed_test_run.status 对齐）。 */
export type SpeedTestRunStatus = "running" | "completed" | "cancelled" | "interrupted";

/** 结果行状态：pending 只会在运行中出现，运行结束后剩余行转 cancelled。 */
export type SpeedTestRowStatus = "pending" | "done" | "failed" | "cancelled";

export interface SpeedTestRun {
	id: number;
	status: SpeedTestRunStatus;
	startedAt: number;
	finishedAt?: number | null;
	total: number;
	/** 内存槽位真相：进程重启后库里可能残留 running，但此标志为 false。 */
	running: boolean;
	/** 本次运行使用的筛选快照，用于判断结果是否属于当前筛选。 */
	providerIds: number[];
	vmIds: number[];
	keyword: string;
}

export interface SpeedTestResultRow {
	providerId: number;
	providerName: string;
	providerModelPk: number;
	modelId: string;
	status: SpeedTestRowStatus;
	ttftMs?: number | null;
	outputTokens?: number | null;
	outputTokensTimeMs?: number | null;
	totalMs?: number | null;
	tps?: number | null;
	requestId?: string | null;
	failReason?: string | null;
	finishedAt?: number | null;
}

export interface SpeedTestRunData {
	run: SpeedTestRun | null;
	results: SpeedTestResultRow[];
}

export interface SpeedTestFilter {
	providerIds: number[];
	vmIds: number[];
	keyword: string;
}

export interface ProviderPreview {
	providerId: number;
	providerName: string;
	count: number;
}

/** 候选模型行：筛选一变化就展示，不必等跑过测速。 */
export interface CandidateModel {
	providerId: number;
	providerName: string;
	providerModelPk: number;
	modelId: string;
	/** 引用该模型的虚拟模型 display_id（已排序去重）。 */
	virtualModels: string[];
}

export interface CandidatePreview {
	total: number;
	providers: ProviderPreview[];
	models: CandidateModel[];
}

export interface StartSpeedTestResult {
	alreadyRunning: boolean;
	runId?: number;
	total?: number;
}

export const speedTestKeys = {
	all: ["speed-test"] as const,
	run: () => ["speed-test", "run"] as const,
	candidates: (filter: SpeedTestFilter) =>
		["speed-test", "candidates", filter.providerIds, filter.vmIds, filter.keyword] as const,
};

/** 过滤条件序列化：空数组/空串 = 该维度不过滤（与后端 resolve 同语义）。 */
function buildFilterQuery(filter: SpeedTestFilter): string {
	const params = new URLSearchParams();
	if (filter.providerIds.length) params.set("providerIds", filter.providerIds.join(","));
	if (filter.vmIds.length) params.set("vmIds", filter.vmIds.join(","));
	const keyword = filter.keyword.trim();
	if (keyword) params.set("keyword", keyword);
	const qs = params.toString();
	return qs ? `?${qs}` : "";
}

/** 候选预览：「将测 N 个模型」与实际会跑的模型同源（后端 resolve 单一事实源）。 */
export function useSpeedTestCandidates(filter: SpeedTestFilter) {
	return useQuery<CandidatePreview>({
		queryKey: speedTestKeys.candidates(filter),
		queryFn: async () => {
			const res = await api
				.get(`speed-test/candidates${buildFilterQuery(filter)}`)
				.json<ApiResponse<CandidatePreview>>();
			return unwrap(res);
		},
		placeholderData: (prev) => prev,
	});
}

/**
 * 当前/上次运行。运行中每 2 秒轮询，终态停止——切页面回来直接复用缓存并
 * 恢复轮询，不需要 SSE 长连接（全站唯一轮询先例同样走 refetchInterval）。
 */
export function useSpeedTestRun() {
	return useQuery<SpeedTestRunData>({
		queryKey: speedTestKeys.run(),
		queryFn: async () => {
			const res = await api.get("speed-test/run").json<ApiResponse<SpeedTestRunData>>();
			return unwrap(res);
		},
		refetchInterval: (query) => {
			const run = query.state.data?.run;
			return run?.status === "running" ? 2000 : false;
		},
	});
}

export function useStartSpeedTest() {
	const queryClient = useQueryClient();
	return useMutation<StartSpeedTestResult, Error, SpeedTestFilter>({
		mutationFn: async (filter) => {
			const res = await api
				.post("speed-test/run", { json: filter })
				.json<ApiResponse<StartSpeedTestResult>>();
			return unwrap(res);
		},
		onSuccess: async () => {
			// 立刻重取一次：轮询靠 status=running 自举，若首次取数仍是旧终态就不会再轮询。
			await queryClient.invalidateQueries({ queryKey: speedTestKeys.run() });
		},
	});
}

export function useCancelSpeedTest() {
	const queryClient = useQueryClient();
	return useMutation<{ cancelled: boolean }, Error, void>({
		mutationFn: async () => {
			const res = await api
				.post("speed-test/run/cancel")
				.json<ApiResponse<{ cancelled: boolean }>>();
			return unwrap(res);
		},
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: speedTestKeys.run() });
		},
	});
}
