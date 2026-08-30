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

export interface ProviderModelRankItem {
	/** 实际服务的供应商 ID。 */
	providerId: number;
	/** 实际服务的供应商名称（供应商已删除时为空串）。 */
	providerName: string;
	/** 模型 ID（供应商侧真实 ID；provider_model 行已删时退化为原始串）。 */
	modelId: string;
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

export interface ProviderModelRankResponse {
	startTime: number;
	endTime: number;
	items: ProviderModelRankItem[];
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

export const providerModelRaceKeys = {
	rank: (window: RaceWindow, sort: RaceSort, providerId?: number) =>
		[
			"stats",
			"provider-model-rank",
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
			providerId ?? null,
		] as const,
};

/**
 * 供应商模型平铺赛马排行查询（全部供应商×模型 + 后端排序；可选按供应商过滤）。
 * @param window 时间窗口
 * @param sort 排序指标与方向
 * @param enabled 是否启用（配合懒加载 useInView 使用，未进入视口不发请求）
 * @param providerId 可选：只返回该供应商的模型
 */
export function useProviderModelRace(
	window: RaceWindow,
	sort: RaceSort,
	enabled: boolean,
	providerId?: number,
) {
	return useQuery<ProviderModelRankResponse>({
		queryKey: providerModelRaceKeys.rank(window, sort, providerId),
		queryFn: async () => {
			const params = new URLSearchParams({
				sortBy: sort.sortBy,
				sortOrder: sort.sortOrder,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			if (providerId !== undefined) {
				params.set("providerId", String(providerId));
			}
			const res = await api
				.get(`stats/provider-model-rank?${params.toString()}`)
				.json<ApiResponse<ProviderModelRankResponse>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免图表闪回骨架导致页面抖动。
		placeholderData: keepPreviousData,
	});
}
