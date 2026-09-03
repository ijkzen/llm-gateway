import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

export interface UsageWindow {
	window: "five_hour" | "daily" | "weekly" | "monthly";
	available: boolean;
	usedPercent?: number;
	remainingPercent?: number;
	resetsAt?: string;
	used?: number;
	limit?: number;
	unit?: string;
	/** 所属容量容器标注（如商汤积分池名）。 */
	label?: string;
}

export interface BalanceItem {
	label: string;
	amount: number;
	currency?: string;
}

export interface ProviderUsage {
	providerId: number;
	fetchedAt: string;
	kind: "quota" | "balance";
	plan?: string;
	windows?: UsageWindow[];
	balances?: BalanceItem[];
}

/**
 * 查询供应商用量。进入详情自动拉取（服务端 60s 缓存）；
 * `refreshToken > 0` 时带 `?refresh=1` 绕过缓存（手动刷新按钮递增 token）。
 */
export function useProviderUsage(id: number | null, refreshToken = 0) {
	return useQuery<ProviderUsage>({
		queryKey: ["provider-usage", id, refreshToken],
		queryFn: async () => {
			const suffix = refreshToken > 0 ? "?refresh=1" : "";
			const res = await api
				.get(`providers/${id}/usage${suffix}`)
				.json<ApiResponse<ProviderUsage>>();
			return unwrap(res);
		},
		enabled: id !== null,
		retry: false,
	});
}
