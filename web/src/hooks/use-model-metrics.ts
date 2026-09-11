import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { QueryWindow } from "@/lib/race-period";
import type { RaceSort, RaceSortKey } from "@/lib/race-types";

export type { RaceSort, RaceSortKey };

export interface ModelMetrics {
	/** 供应商 ID。 */
	providerId: number;
	/** 供应商名称（供应商已删除时为空串）。 */
	providerName: string;
	/** 模型 ID（供应商侧真实 ID）。 */
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

export const modelMetricsKeys = {
	metrics: (providerId: number, modelId: string, window: QueryWindow) =>
		statsKey("model-metrics", [providerId, modelId, ...window.key]),
};

/**
 * 单模型指标查询（模型详情三级页指标卡片用）。
 * @param providerId 供应商 ID
 * @param modelId 模型 ID
 * @param window 取数窗口（key 用稳定身份，绝对起止在取数时解析）
 * @param enabled 是否启用
 */
export function useModelMetrics(
	providerId: number,
	modelId: string,
	window: QueryWindow,
	enabled = true,
) {
	return statsQuery<ModelMetrics>({
		endpoint: "stats/model-metrics",
		key: modelMetricsKeys.metrics(providerId, modelId, window),
		window,
		enabled,
		params: {
			providerId,
			modelId,
		},
	});
}
