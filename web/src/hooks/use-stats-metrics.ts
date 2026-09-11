import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { QueryWindow } from "@/lib/race-period";

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

export const statsMetricsKeys = {
	provider: (providerId: number, window: QueryWindow) =>
		statsKey("provider-metrics", [providerId, ...window.key]),
	virtualModel: (virtualModelId: number, window: QueryWindow) =>
		statsKey("virtual-model-metrics", [virtualModelId, ...window.key]),
};

/** 供应商级 6 指标聚合（二级页顶部指标卡）。 */
export function useProviderMetrics(providerId: number, window: QueryWindow, enabled = true) {
	return statsQuery<ProviderMetrics>({
		endpoint: "stats/provider-metrics",
		key: statsMetricsKeys.provider(providerId, window),
		window,
		enabled,
		params: {
			providerId,
		},
	});
}

/** 虚拟模型级 6 指标聚合（二级页顶部指标卡）。 */
export function useVirtualModelMetrics(
	virtualModelId: number,
	window: QueryWindow,
	enabled = true,
) {
	return statsQuery<VirtualModelMetrics>({
		endpoint: "stats/virtual-model-metrics",
		key: statsMetricsKeys.virtualModel(virtualModelId, window),
		window,
		enabled,
		params: {
			virtualModelId,
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

export function useApiKeyMetrics(apiKey: string | null, window: QueryWindow, enabled = true) {
	return statsQuery<ApiKeyMetrics>({
		endpoint: "stats/api-key-metrics",
		key: statsKey("api-key-metrics", [apiKey, ...window.key]),
		window,
		enabled: enabled && apiKey !== null,
		params: {
			apiKey: apiKey ?? undefined,
		},
	});
}
