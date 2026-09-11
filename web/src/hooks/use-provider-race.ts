import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { QueryWindow } from "@/lib/race-period";
import type { RaceSort, RaceSortKey } from "@/lib/race-types";

export type { RaceSort, RaceSortKey };

export interface ProviderRankItem {
	/** 实际服务的供应商 ID。 */
	providerId: number;
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

export const providerRaceKeys = {
	rank: (window: QueryWindow, sort: RaceSort, apiKey?: string) =>
		statsKey("provider-rank", [...window.key, sort.sortBy, sort.sortOrder, apiKey]),
};

/**
 * 供应商赛马排行查询（全部供应商 + 后端排序；可选按调用方 API Key 过滤）。
 * @param window 取数窗口（key 用稳定身份，绝对起止在取数时解析）
 * @param sort 排序指标与方向
 * @param enabled 是否启用（配合懒加载 useInView 使用，未进入视口不发请求）
 * @param apiKey 可选：只统计该调用方 API Key 的请求（API Key 数据面板用）
 */
export function useProviderRace(
	window: QueryWindow,
	sort: RaceSort,
	enabled: boolean,
	apiKey?: string,
) {
	return statsQuery<ProviderRankResponse>({
		endpoint: "stats/provider-rank",
		key: providerRaceKeys.rank(window, sort, apiKey),
		window,
		enabled,
		params: {
			sortBy: sort.sortBy,
			sortOrder: sort.sortOrder,
			apiKey,
		},
	});
}
