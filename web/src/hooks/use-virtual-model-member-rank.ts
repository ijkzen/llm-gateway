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

export const virtualModelMemberRankKeys = {
	rank: (window: RaceWindow, sort: RaceSort, virtualModelId: number) =>
		[
			"stats",
			"virtual-model-member-rank",
			window.startTime,
			window.endTime,
			sort.sortBy,
			sort.sortOrder,
			virtualModelId,
		] as const,
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
	return useQuery<VirtualModelMemberRankResponse>({
		queryKey: virtualModelMemberRankKeys.rank(window, sort, virtualModelId),
		queryFn: async () => {
			const params = new URLSearchParams({
				sortBy: sort.sortBy,
				sortOrder: sort.sortOrder,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
				virtualModelId: String(virtualModelId),
			});
			const res = await api
				.get(`stats/virtual-model-member-rank?${params.toString()}`)
				.json<ApiResponse<VirtualModelMemberRankResponse>>();
			return unwrap(res);
		},
		enabled,
	});
}
