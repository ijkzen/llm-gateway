import { type ApiResponse, api, unwrap } from "@/lib/api";
import type { ChartGranularity } from "@/lib/race-period";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

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

/** 失败原因分布条目（reason 为空串表示「无原因」）。 */
export interface FailureReasonItem {
	reason: string;
	count: number;
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
	failureReasons: FailureReasonItem[];
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
export interface InsightParams {
	startTime?: number;
	endTime?: number;
	providerId?: number;
	virtualModelId?: number;
	modelId?: string;
	/** 按调用方 API Key 名称过滤（可选；API Key 数据面板用）。 */
	apiKey?: string;
	granularity?: ChartGranularity;
	tzOffsetMinutes?: number;
}

export const insightKeys = {
	all: (params: InsightParams = {}) =>
		[
			"stats",
			"insight",
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

export function useDashboardInsight(params: InsightParams = {}, enabled = true) {
	return useQuery<InsightData>({
		queryKey: insightKeys.all(params),
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
			const res = await api.get(`stats/insight${suffix}`).json<ApiResponse<InsightData>>();
			return unwrap(res);
		},
		enabled,
		// 切换时间窗口期间保留上一窗口数据，避免图表闪回骨架导致页面抖动。
		placeholderData: keepPreviousData,
	});
}
