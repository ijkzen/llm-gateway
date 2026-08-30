import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

/** 赛马排序指标。 */
export type RaceSortKey =
	| "totalTokens"
	| "requestCount"
	| "ttft"
	| "requestTime"
	| "tps"
	| "cacheHitRate";

export interface ProviderRankItem {
	/** 实际服务的供应商名称（供应商已删除时为空串）。 */
	providerName: string;
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

export interface ProviderRankResponse {
	startTime: number;
	endTime: number;
	items: ProviderRankItem[];
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

export const providerRaceKeys = {
	rank: (window: RaceWindow, sort: RaceSort) =>
		[
			"stats",
			"provider-rank",
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
		] as const,
};

/**
 * 供应商赛马排行查询（全部供应商 + 后端排序）。
 * @param window 时间窗口
 * @param sort 排序指标与方向
 * @param enabled 是否启用（配合懒加载 useInView 使用，未进入视口不发请求）
 */
export function useProviderRace(window: RaceWindow, sort: RaceSort, enabled: boolean) {
	return useQuery<ProviderRankResponse>({
		queryKey: providerRaceKeys.rank(window, sort),
		queryFn: async () => {
			const params = new URLSearchParams({
				sortBy: sort.sortBy,
				sortOrder: sort.sortOrder,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			const res = await api
				.get(`stats/provider-rank?${params.toString()}`)
				.json<ApiResponse<ProviderRankResponse>>();
			return unwrap(res);
		},
		enabled,
	});
}
