import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { QueryWindow } from "@/lib/race-period";
import type { RaceSort, RaceSortKey } from "@/lib/race-types";

export type { RaceSort, RaceSortKey };

export interface VirtualModelMemberRankItem {
	/** 成员所属供应商 ID。 */
	providerId: number;
	/** 成员所属供应商名称（供应商已删除时为空串）。 */
	providerName: string;
	/** 成员模型 ID（供应商侧真实 ID）。 */
	modelId: string;
	/** provider_model 自增主键（成员恒指向现存模型，恒非空）。 */
	modelPk: number | null;
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
	rank: (window: QueryWindow, sort: RaceSort, virtualModelId: number) =>
		statsKey("virtual-model-member-rank", [
			...window.key,
			sort.sortBy,
			sort.sortOrder,
			virtualModelId,
		]),
};

/**
 * 虚拟模型成员模型排行查询（配置成员全量 + 后端排序）。
 * @param window 取数窗口（key 用稳定身份，绝对起止在取数时解析）
 * @param sort 排序指标与方向
 * @param enabled 是否启用
 * @param virtualModelId 虚拟模型 ID（必填）
 */
export function useVirtualModelMemberRank(
	window: QueryWindow,
	sort: RaceSort,
	enabled: boolean,
	virtualModelId: number,
) {
	return statsQuery<VirtualModelMemberRankResponse>({
		endpoint: "stats/virtual-model-member-rank",
		key: virtualModelMemberRankKeys.rank(window, sort, virtualModelId),
		window,
		enabled,
		params: {
			sortBy: sort.sortBy,
			sortOrder: sort.sortOrder,
			virtualModelId,
		},
	});
}
