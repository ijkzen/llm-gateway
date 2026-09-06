import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { RaceSort, RaceSortKey, RaceWindow } from "@/lib/race-types";

export type { RaceSort, RaceSortKey, RaceWindow };

export interface VirtualModelMemberRankItem {
	/** 成员所属供应商 ID。 */
	providerId: number;
	/** 成员所属供应商名称（供应商已删除时为空串）。 */
	providerName: string;
	/** 成员模型 ID（供应商侧真实 ID）。 */
	modelId: string;
	/** 成员是否启用（virtual_model_item.enable）。 */
	memberEnable: boolean;
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

export interface VirtualModelMemberRankResponse {
	startTime: number;
	endTime: number;
	items: VirtualModelMemberRankItem[];
}

export const virtualModelMemberRankKeys = {
	rank: (window: RaceWindow, sort: RaceSort, virtualModelId: number) =>
		statsKey("virtual-model-member-rank", [
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
			virtualModelId,
		]),
};

/**
 * 虚拟模型成员模型排行查询（配置成员全量 + 后端排序）。
 * @param window 时间窗口
 * @param sort 排序指标与方向
 * @param enabled 是否启用
 * @param virtualModelId 虚拟模型 ID（必填）
 */
export function useVirtualModelMemberRank(
	window: RaceWindow,
	sort: RaceSort,
	enabled: boolean,
	virtualModelId: number,
) {
	return statsQuery<VirtualModelMemberRankResponse>({
		endpoint: "stats/virtual-model-member-rank",
		key: virtualModelMemberRankKeys.rank(window, sort, virtualModelId),
		enabled,
		params: {
			sortBy: sort.sortBy,
			sortOrder: sort.sortOrder,
			startTime: window.startTime,
			endTime: window.endTime,
			virtualModelId,
		},
	});
}
