import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

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

export interface RaceWindow {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime: number;
}

export const modelMetricsKeys = {
	metrics: (providerId: number, modelId: string, window: RaceWindow) =>
		["stats", "model-metrics", providerId, modelId, window.startTime, window.endTime] as const,
};

/**
 * 单模型指标查询（模型详情三级页指标卡片用）。
 * @param providerId 供应商 ID
 * @param modelId 模型 ID
 * @param window 时间窗口
 * @param enabled 是否启用
 */
export function useModelMetrics(
	providerId: number,
	modelId: string,
	window: RaceWindow,
	enabled = true,
) {
	return useQuery<ModelMetrics>({
		queryKey: modelMetricsKeys.metrics(providerId, modelId, window),
		queryFn: async () => {
			const params = new URLSearchParams({
				providerId: String(providerId),
				modelId,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			const res = await api
				.get(`stats/model-metrics?${params.toString()}`)
				.json<ApiResponse<ModelMetrics>>();
			return unwrap(res);
		},
		enabled,
	});
}
