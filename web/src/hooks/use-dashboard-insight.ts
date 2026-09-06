import { statsKey, statsQuery } from "@/hooks/stats-query";
import type { ChartGranularity } from "@/lib/race-period";
import type { StatsFilter, TimeWindowParams } from "@/lib/race-types";

/** 每桶趋势点（整数，如调用数/Token 数/失败数）。 */
export interface TrendPoint {
	bucketStart: number;
	value: number;
}

/** 每桶浮点趋势点（比率/速率，如失败率/缓存命中率/每秒输出 token）。 */
export interface FloatTrendPoint {
	bucketStart: number;
	value: number;
}

/** 每桶延迟分位点（毫秒；无样本桶字段为 0）。 */
export interface PercentilePoint {
	bucketStart: number;
	p50: number;
	p90: number;
	p95: number;
	p99: number;
}

/** 按 API Key 聚合的调用量条目。 */
export interface ApiKeyRankItem {
	apiKeyName: string;
	value: number;
}

/** /api/stats/insight 响应：性能与可靠性分析四组数据。 */
export interface InsightData {
	/** 每桶全部调用数（成功+失败；失败趋势图堆叠基准）。 */
	callTrend: TrendPoint[];
	failureTrend: TrendPoint[];
	failureRateTrend: FloatTrendPoint[];
	ttftPercentiles: PercentilePoint[];
	latencyPercentiles: PercentilePoint[];
	inputTokenTrend: TrendPoint[];
	outputTokenTrend: TrendPoint[];
	cacheHitRateTrend: FloatTrendPoint[];
	outputTokensPerSecTrend: FloatTrendPoint[];
	apiKeyRank: ApiKeyRankItem[];
	rpmTrend: TrendPoint[];
	/** 吞吐：每分钟 token 量（Tokens Per Minute）。 */
	tpmTrend: FloatTrendPoint[];
	streamRatioTrend: FloatTrendPoint[];
}

/** 查询参数（与 useDashboardCharts 同一套过滤/窗口/粒度）。 */
export interface InsightParams extends TimeWindowParams, StatsFilter {
	granularity?: ChartGranularity;
	tzOffsetMinutes?: number;
}

export const insightKeys = {
	all: (params: InsightParams = {}) =>
		statsKey("insight", [
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

export function useDashboardInsight(params: InsightParams = {}, enabled = true) {
	return statsQuery<InsightData>({
		endpoint: "stats/insight",
		key: insightKeys.all(params),
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
