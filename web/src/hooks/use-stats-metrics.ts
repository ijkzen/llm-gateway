import { statsKey, statsQuery } from "@/hooks/stats-query";

/** 供应商级 6 指标（与后端 GET /api/stats/provider-metrics 对齐）。 */
export interface ProviderMetrics {
	providerId: number;
	providerName: string;
	requestCount: number;
	totalTokens: number;
	ttft: number;
	requestTime: number;
	tps: number;
	cacheHitRate: number;
}

/** 虚拟模型级 6 指标（与后端 GET /api/stats/virtual-model-metrics 对齐）。 */
export interface VirtualModelMetrics {
	virtualModelId: number;
	virtualModelDisplayId: string;
	requestCount: number;
	totalTokens: number;
	ttft: number;
	requestTime: number;
	tps: number;
	cacheHitRate: number;
}

export interface MetricsWindow {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime: number;
}

export const statsMetricsKeys = {
	provider: (providerId: number, window: MetricsWindow) =>
		statsKey("provider-metrics", [providerId, window.startTime, window.endTime]),
	virtualModel: (virtualModelId: number, window: MetricsWindow) =>
		statsKey("virtual-model-metrics", [virtualModelId, window.startTime, window.endTime]),
};

/** 供应商级 6 指标聚合（二级页顶部指标卡）。 */
export function useProviderMetrics(providerId: number, window: MetricsWindow, enabled = true) {
	return statsQuery<ProviderMetrics>({
		endpoint: "stats/provider-metrics",
		key: statsMetricsKeys.provider(providerId, window),
		enabled,
		params: {
			providerId,
			startTime: window.startTime,
			endTime: window.endTime,
		},
	});
}

/** 虚拟模型级 6 指标聚合（二级页顶部指标卡）。 */
export function useVirtualModelMetrics(
	virtualModelId: number,
	window: MetricsWindow,
	enabled = true,
) {
	return statsQuery<VirtualModelMetrics>({
		endpoint: "stats/virtual-model-metrics",
		key: statsMetricsKeys.virtualModel(virtualModelId, window),
		enabled,
		params: {
			virtualModelId,
			startTime: window.startTime,
			endTime: window.endTime,
		},
	});
}

/** API Key 级 6 指标（与 GET /api/stats/api-key-metrics 对齐；API Key 数据面板顶部指标卡）。 */
export interface ApiKeyMetrics {
	/** 调用方 API Key 名称。 */
	apiKeyName: string;
	requestCount: number;
	totalTokens: number;
	ttft: number;
	requestTime: number;
	tps: number;
	cacheHitRate: number;
}

export function useApiKeyMetrics(apiKey: string | null, window: MetricsWindow, enabled = true) {
	return statsQuery<ApiKeyMetrics>({
		endpoint: "stats/api-key-metrics",
		key: statsKey("api-key-metrics", [apiKey, window.startTime, window.endTime]),
		enabled: enabled && apiKey !== null,
		params: {
			apiKey: apiKey ?? undefined,
			startTime: window.startTime,
			endTime: window.endTime,
		},
	});
}
