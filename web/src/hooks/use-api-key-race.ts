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

export interface ApiKeyRankItem {
	/** 调用方 API Key 名称（Key 已删除的历史行仍按原名聚合）。 */
	apiKeyName: string;
	/** 现存 API Key 的数字主键（后端 LEFT JOIN api_key 补出；Key 已删除时为 null）。 */
	apiKeyId?: number | null;
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

export interface ApiKeyRankResponse {
	startTime: number;
	endTime: number;
	items: ApiKeyRankItem[];
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

/** API Key 赛马的过滤维度：三级页（模型详情）用 providerId + modelId。 */
export interface ApiKeyRaceFilter {
	/** 二级页（供应商详情）：只统计该供应商。 */
	providerId?: number;
	/** 二级页（虚拟模型详情）：只统计该虚拟模型。 */
	virtualModelId?: number;
	/** 三级页（模型详情）：只统计该模型（须与 providerId 同传）。 */
	modelId?: string;
}

export const apiKeyRaceKeys = {
	rank: (window: RaceWindow, sort: RaceSort, filter?: ApiKeyRaceFilter) =>
		[
			"stats",
			"api-key-rank",
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
			filter?.providerId ?? null,
			filter?.virtualModelId ?? null,
			filter?.modelId ?? null,
		] as const,
};

/**
 * API Key 维度赛马排行查询（全部 API Key + 后端排序；可选按供应商/虚拟模型/模型过滤）。
 * @param window 时间窗口
 * @param sort 排序指标与方向
 * @param enabled 是否启用（配合懒加载 useInView 使用，未进入视口不发请求）
 * @param filter 可选过滤（首页传空，二级/三级页按需传）
 */
export function useApiKeyRace(
	window: RaceWindow,
	sort: RaceSort,
	enabled: boolean,
	filter?: ApiKeyRaceFilter,
) {
	return useQuery<ApiKeyRankResponse>({
		queryKey: apiKeyRaceKeys.rank(window, sort, filter),
		queryFn: async () => {
			const params = new URLSearchParams({
				sortBy: sort.sortBy,
				sortOrder: sort.sortOrder,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			if (filter?.providerId !== undefined) {
				params.set("providerId", String(filter.providerId));
			}
			if (filter?.virtualModelId !== undefined) {
				params.set("virtualModelId", String(filter.virtualModelId));
			}
			if (filter?.modelId !== undefined) {
				params.set("modelId", filter.modelId);
			}
			const res = await api
				.get(`stats/api-key-rank?${params.toString()}`)
				.json<ApiResponse<ApiKeyRankResponse>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免图表闪回骨架导致页面抖动。
		placeholderData: keepPreviousData,
	});
}
