import { type ApiResponse, api, unwrap } from "@/lib/api";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

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
		["stats", "provider-metrics", providerId, window.startTime, window.endTime] as const,
	virtualModel: (virtualModelId: number, window: MetricsWindow) =>
		["stats", "virtual-model-metrics", virtualModelId, window.startTime, window.endTime] as const,
};

/** 供应商级 6 指标聚合（二级页顶部指标卡）。 */
export function useProviderMetrics(providerId: number, window: MetricsWindow, enabled = true) {
	return useQuery<ProviderMetrics>({
		queryKey: statsMetricsKeys.provider(providerId, window),
		queryFn: async () => {
			const params = new URLSearchParams({
				providerId: String(providerId),
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			const res = await api
				.get(`stats/provider-metrics?${params.toString()}`)
				.json<ApiResponse<ProviderMetrics>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免指标卡片闪回骨架。
		placeholderData: keepPreviousData,
	});
}

/** 虚拟模型级 6 指标聚合（二级页顶部指标卡）。 */
export function useVirtualModelMetrics(
	virtualModelId: number,
	window: MetricsWindow,
	enabled = true,
) {
	return useQuery<VirtualModelMetrics>({
		queryKey: statsMetricsKeys.virtualModel(virtualModelId, window),
		queryFn: async () => {
			const params = new URLSearchParams({
				virtualModelId: String(virtualModelId),
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			const res = await api
				.get(`stats/virtual-model-metrics?${params.toString()}`)
				.json<ApiResponse<VirtualModelMetrics>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免指标卡片闪回骨架。
		placeholderData: keepPreviousData,
	});
}
