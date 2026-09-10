import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

/** 订阅周期 Token 预估（与后端 GET /api/providers/{id}/usage/estimate 对齐）。 */
export interface UsageEstimate {
	providerId: number;
	/** 用于预估的窗口：weekly / monthly / none。 */
	window: string;
	/** 窗口起点到 now（不越过 resets_at）之间请求表统计的已用 token。 */
	usedTokens: number;
	/** 用量卡该窗口已用配额（厂商单位）。 */
	used?: number | null;
	/** 用量卡该窗口总配额（厂商单位）。 */
	limit?: number | null;
	/** 预估订阅周期内可用 token 总量（可预估时才有）。 */
	estimatedTotalTokens?: number | null;
	/** 是否可预估（用量卡可折算比例且网关已记录 token）。 */
	estimatable: boolean;
}

export const usageEstimateKeys = {
	estimate: (providerId: number) => ["provider-usage", "estimate", providerId] as const,
};

/** 订阅制供应商的订阅周期 Token 总量预估查询。 */
export function useUsageEstimate(providerId: number | null) {
	return useQuery<UsageEstimate>({
		queryKey: usageEstimateKeys.estimate(providerId ?? -1),
		queryFn: async () => {
			const res = await api
				// 内部会触发真实用量抓取（上游超时 15s），前端须留出余量（19-04）。
				.get(`providers/${providerId}/usage/estimate`, { timeout: 30000 })
				.json<ApiResponse<UsageEstimate>>();
			return unwrap(res);
		},
		enabled: providerId !== null,
		retry: false,
	});
}
