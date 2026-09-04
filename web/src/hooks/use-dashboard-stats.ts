import { type ApiResponse, api, unwrap } from "@/lib/api";
import type { ChartGranularity } from "@/lib/race-period";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

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
	/** 按模型 ID 过滤（可选；供应商侧真实模型 ID）。 */
	modelId?: string;
	/** 按调用方 API Key 名称过滤（可选；API Key 数据面板用）。 */
	apiKey?: string;
	/** 桶粒度（hour/day/month/year）。缺省由后端按窗口长度回退推断。 */
	granularity?: ChartGranularity;
	/** 客户端 UTC 偏移（分钟，东八区 480）。与 granularity 搭配使用。 */
	tzOffsetMinutes?: number;
}

/** 累计指标查询参数（可选时间窗口；缺省返回全历史累计）。 */
export interface SummaryParams {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime?: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime?: number;
}

export const dashboardStatsKeys = {
	summary: (params: SummaryParams = {}) =>
		["stats", "summary", params.startTime ?? null, params.endTime ?? null] as const,
	charts: (params: ChartsParams = {}) =>
		[
			"stats",
			"charts",
			params.startTime ?? null,
			params.endTime ?? null,
			params.providerId ?? null,
			params.virtualModelId ?? null,
			params.modelId ?? null,
			params.apiKey ?? null,
			params.granularity ?? null,
			params.tzOffsetMinutes ?? null,
		] as const,
};

// 数据面板不做主动轮询刷新：一级/二级/三级页均依赖手动刷新或切窗触发
// （refetchOnWindowFocus 等默认策略保持不变）。

export function useDashboardSummary(params: SummaryParams = {}) {
	return useQuery<DashboardSummary>({
		queryKey: dashboardStatsKeys.summary(params),
		queryFn: async () => {
			const query = new URLSearchParams();
			if (params.startTime !== undefined) query.set("startTime", String(params.startTime));
			if (params.endTime !== undefined) query.set("endTime", String(params.endTime));
			const suffix = query.size > 0 ? `?${query.toString()}` : "";
			const res = await api.get(`stats/summary${suffix}`).json<ApiResponse<DashboardSummary>>();
			return unwrap(res);
		},
	});
}

export function useDashboardCharts(params: ChartsParams = {}, enabled = true) {
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
			if (params.modelId !== undefined) query.set("modelId", params.modelId);
			if (params.apiKey !== undefined) query.set("apiKey", params.apiKey);
			if (params.granularity !== undefined) query.set("granularity", params.granularity);
			if (params.tzOffsetMinutes !== undefined) {
				query.set("tzOffsetMinutes", String(params.tzOffsetMinutes));
			}
			const suffix = query.size > 0 ? `?${query.toString()}` : "";
			const res = await api.get(`stats/charts${suffix}`).json<ApiResponse<DashboardCharts>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免图表闪回骨架导致页面抖动。
		placeholderData: keepPreviousData,
	});
}
