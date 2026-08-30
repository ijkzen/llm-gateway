import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

export interface DashboardSummary {
	totalRequests: number;
	successRate: number;
	totalTokens: number;
	cacheHitRate: number;
}

export interface TrendPoint {
	bucketStart: number;
	value: number;
}

export interface ModelValue {
	/** 实际服务的供应商名称（供应商已删除时为空串）。 */
	providerName: string;
	modelId: string;
	value: number;
}

export interface DashboardCharts {
	callTrend: TrendPoint[];
	callByModel: ModelValue[];
	tokenTrend: TrendPoint[];
	tokenByModel: ModelValue[];
}

/** 图表查询参数（全部可选；缺省回退过去 24 小时）。 */
export interface ChartsParams {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime?: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime?: number;
	/** 按供应商过滤（可选）。 */
	providerId?: number;
	/** 按虚拟模型过滤（可选）。 */
	virtualModelId?: number;
}

export const dashboardStatsKeys = {
	summary: ["stats", "summary"] as const,
	charts: (params: ChartsParams = {}) =>
		[
			"stats",
			"charts",
			params.startTime ?? null,
			params.endTime ?? null,
			params.providerId ?? null,
			params.virtualModelId ?? null,
		] as const,
};

const REFETCH_INTERVAL = 60_000;

export function useDashboardSummary() {
	return useQuery<DashboardSummary>({
		queryKey: dashboardStatsKeys.summary,
		queryFn: async () => {
			const res = await api.get("stats/summary").json<ApiResponse<DashboardSummary>>();
			return unwrap(res);
		},
		refetchInterval: REFETCH_INTERVAL,
	});
}

export function useDashboardCharts(params: ChartsParams = {}) {
	return useQuery<DashboardCharts>({
		queryKey: dashboardStatsKeys.charts(params),
		queryFn: async () => {
			const query = new URLSearchParams();
			if (params.startTime !== undefined) query.set("startTime", String(params.startTime));
			if (params.endTime !== undefined) query.set("endTime", String(params.endTime));
			if (params.providerId !== undefined) query.set("providerId", String(params.providerId));
			if (params.virtualModelId !== undefined) {
				query.set("virtualModelId", String(params.virtualModelId));
			}
			const suffix = query.size > 0 ? `?${query.toString()}` : "";
			const res = await api.get(`stats/charts${suffix}`).json<ApiResponse<DashboardCharts>>();
			return unwrap(res);
		},
		refetchInterval: REFETCH_INTERVAL,
	});
}
