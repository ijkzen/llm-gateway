import { statsFilterKeySegments, statsKey, statsQuery } from "@/hooks/stats-query";
import type { RaceSort, RaceWindow, StatsFilter } from "@/lib/race-types";

export type { RaceSort, RaceSortKey, RaceWindow } from "@/lib/race-types";

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

/** API Key 赛马的过滤维度：三级页（模型详情）用 providerId + modelId。 */
export type ApiKeyRaceFilter = Pick<StatsFilter, "providerId" | "virtualModelId" | "modelId">;

export const apiKeyRaceKeys = {
	rank: (window: RaceWindow, sort: RaceSort, filter?: ApiKeyRaceFilter) =>
		statsKey("api-key-rank", [
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
			...(filter ? statsFilterKeySegments(filter).slice(0, 3) : [null, null, null]),
		]),
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
	return statsQuery<ApiKeyRankResponse>({
		endpoint: "stats/api-key-rank",
		key: apiKeyRaceKeys.rank(window, sort, filter),
		enabled,
		params: {
			sortBy: sort.sortBy,
			sortOrder: sort.sortOrder,
			startTime: window.startTime,
			endTime: window.endTime,
			providerId: filter?.providerId,
			virtualModelId: filter?.virtualModelId,
			modelId: filter?.modelId,
		},
	});
}
