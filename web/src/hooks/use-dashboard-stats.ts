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

export const dashboardStatsKeys = {
	summary: ["stats", "summary"] as const,
	charts: ["stats", "charts"] as const,
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

export function useDashboardCharts() {
	return useQuery<DashboardCharts>({
		queryKey: dashboardStatsKeys.charts,
		queryFn: async () => {
			const res = await api.get("stats/charts").json<ApiResponse<DashboardCharts>>();
			return unwrap(res);
		},
		refetchInterval: REFETCH_INTERVAL,
	});
}
