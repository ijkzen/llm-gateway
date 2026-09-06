import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { ChartGranularity } from "@/lib/race-period";
import type { StatsFilter, TimeWindowParams } from "@/lib/race-types";

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
export interface ChartsParams extends TimeWindowParams, StatsFilter {
	/** 桶粒度（hour/day/month/year）。缺省由后端按窗口长度回退推断。 */
	granularity?: ChartGranularity;
	/** 客户端 UTC 偏移（分钟，东八区 480）。与 granularity 搭配使用。 */
	tzOffsetMinutes?: number;
}

/** 累计指标查询参数（可选时间窗口；缺省返回全历史累计）。 */
export type SummaryParams = TimeWindowParams;

export const dashboardStatsKeys = {
	summary: (params: SummaryParams = {}) => statsKey("summary", [params.startTime, params.endTime]),
	charts: (params: ChartsParams = {}) =>
		statsKey("charts", [
			params.startTime,
			params.endTime,
			params.providerId,
			params.virtualModelId,
			params.modelId,
			params.apiKey,
			params.granularity,
			params.tzOffsetMinutes,
		]),
};

// 数据面板不做主动轮询刷新：一级/二级/三级页均依赖手动刷新或切窗触发
// （refetchOnWindowFocus 等默认策略保持不变）。

export function useDashboardSummary(params: SummaryParams = {}) {
	return statsQuery<DashboardSummary>({
		endpoint: "stats/summary",
		key: dashboardStatsKeys.summary(params),
		params: { startTime: params.startTime, endTime: params.endTime },
		keepPrevious: false,
	});
}

export function useDashboardCharts(params: ChartsParams = {}, enabled = true) {
	return statsQuery<DashboardCharts>({
		endpoint: "stats/charts",
		key: dashboardStatsKeys.charts(params),
		enabled,
		params: {
			startTime: params.startTime,
			endTime: params.endTime,
			providerId: params.providerId,
			virtualModelId: params.virtualModelId,
			modelId: params.modelId,
			apiKey: params.apiKey,
			granularity: params.granularity,
			tzOffsetMinutes: params.tzOffsetMinutes,
		},
	});
}
