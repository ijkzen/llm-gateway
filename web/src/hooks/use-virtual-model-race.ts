import { type ApiResponse, api, unwrap } from "@/lib/api";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

/** 赛马排序指标。 */
export type RaceSortKey =
	| "totalTokens"
	| "requestCount"
	| "ttft"
	| "requestTime"
	| "tps"
	| "cacheHitRate";

export interface VirtualModelRankItem {
	/** 虚拟模型 ID。 */
	virtualModelId: number;
	/** 虚拟模型对外 ID（虚拟模型已删除时为空串）。 */
	virtualModelDisplayId: string;
	/** 成功请求数。 */
	requestCount: number;
	/** 总计 token（成功请求的 total_tokens 合计）。 */
	totalTokens: number;
	/** 流式请求首 token 耗时均值（毫秒）。 */
	ttft: number;
	/** 平均请求耗时（毫秒）。 */
	requestTime: number;
	/** TPS（加权均值）。 */
	tps: number;
	/** 缓存命中率（加权，0~1）。 */
	cacheHitRate: number;
}

export interface VirtualModelRankResponse {
	startTime: number;
	endTime: number;
	items: VirtualModelRankItem[];
}

export interface RaceWindow {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime: number;
}

export interface RaceSort {
	sortBy: RaceSortKey;
	sortOrder: "asc" | "desc";
}

export const virtualModelRaceKeys = {
	rank: (window: RaceWindow, sort: RaceSort, apiKey?: string) =>
		[
			"stats",
			"virtual-model-rank",
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
			apiKey ?? null,
		] as const,
};

/**
 * 虚拟模型赛马排行查询（全部虚拟模型 + 后端排序；可选按调用方 API Key 过滤）。
 * @param window 时间窗口
 * @param sort 排序指标与方向
 * @param enabled 是否启用（配合懒加载 useInView 使用，未进入视口不发请求）
 * @param apiKey 可选：只统计该调用方 API Key 的请求（API Key 数据面板用）
 */
export function useVirtualModelRace(
	window: RaceWindow,
	sort: RaceSort,
	enabled: boolean,
	apiKey?: string,
) {
	return useQuery<VirtualModelRankResponse>({
		queryKey: virtualModelRaceKeys.rank(window, sort, apiKey),
		queryFn: async () => {
			const params = new URLSearchParams({
				sortBy: sort.sortBy,
				sortOrder: sort.sortOrder,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			if (apiKey !== undefined) {
				params.set("apiKey", apiKey);
			}
			const res = await api
				.get(`stats/virtual-model-rank?${params.toString()}`)
				.json<ApiResponse<VirtualModelRankResponse>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免图表闪回骨架导致页面抖动。
		placeholderData: keepPreviousData,
	});
}
