import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { ChartGranularity, QueryWindow } from "@/lib/race-period";
import type { StatsFilter } from "@/lib/race-types";

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
export interface ChartsParams extends StatsFilter {
	/** 桶粒度（hour/day/month/year）。缺省由后端按窗口长度回退推断。 */
	granularity?: ChartGranularity;
	/** 取数窗口；缺省由后端回退默认窗口。 */
	window?: QueryWindow;
}

/** 累计指标查询参数（可选时间窗口；缺省返回全历史累计）。 */
export interface SummaryParams {
	/** 取数窗口；缺省返回全历史累计。 */
	window?: QueryWindow;
}

export const dashboardStatsKeys = {
	summary: (params: SummaryParams = {}) => statsKey("summary", [...(params.window?.key ?? [])]),
	charts: (params: ChartsParams = {}) =>
		statsKey("charts", [
			...(params.window?.key ?? []),
			params.providerId,
			params.virtualModelId,
			params.modelId,
			params.apiKey,
			params.granularity,
		]),
};

// 数据面板不做主动轮询刷新：一级/二级/三级页均依赖手动刷新或切窗触发
// （refetchOnWindowFocus 等默认策略保持不变）。

export function useDashboardSummary(params: SummaryParams = {}) {
	return statsQuery<DashboardSummary>({
		endpoint: "stats/summary",
		key: dashboardStatsKeys.summary(params),
		window: params.window,
		keepPrevious: false,
		params: {},
	});
}

export function useDashboardCharts(params: ChartsParams = {}, enabled = true) {
	return statsQuery<DashboardCharts>({
		endpoint: "stats/charts",
		key: dashboardStatsKeys.charts(params),
		window: params.window,
		enabled,
		params: {
			providerId: params.providerId,
			virtualModelId: params.virtualModelId,
			modelId: params.modelId,
			apiKey: params.apiKey,
			granularity: params.granularity,
		},
	});
}
