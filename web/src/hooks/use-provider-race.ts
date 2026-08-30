import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

/** 赛马指标。 */
export type RaceMetric = "token" | "tps" | "ttft";

export interface ProviderRankItem {
	/** 实际服务的供应商名称（供应商已删除时为空串）。 */
	providerName: string;
	requestCount: number;
	value: number;
}

export interface ProviderRankResponse {
	metric: RaceMetric;
	startTime: number;
	endTime: number;
	items: ProviderRankItem[];
}

export interface RaceWindow {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime: number;
}

export const providerRaceKeys = {
	rank: (metric: RaceMetric, window: RaceWindow) =>
		["stats", "provider-rank", metric, window.startTime, window.endTime] as const,
};

/**
 * 供应商赛马排行查询。
 * @param metric 指标
 * @param window 时间窗口
 * @param enabled 是否启用（配合懒加载 useInView 使用，未进入视口不发请求）
 */
export function useProviderRace(metric: RaceMetric, window: RaceWindow, enabled: boolean) {
	return useQuery<ProviderRankResponse>({
		queryKey: providerRaceKeys.rank(metric, window),
		queryFn: async () => {
			const params = new URLSearchParams({
				metric,
				startTime: String(window.startTime),
				endTime: String(window.endTime),
			});
			const res = await api
				.get(`stats/provider-rank?${params.toString()}`)
				.json<ApiResponse<ProviderRankResponse>>();
			return unwrap(res);
		},
		enabled,
	});
}
