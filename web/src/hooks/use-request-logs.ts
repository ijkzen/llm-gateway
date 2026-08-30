import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

/** 请求日志行（与后端 GET /api/request-logs 的 items 对齐）。 */
export interface RequestLogRow {
	requestId: string;
	virtualModelId: number;
	virtualModelDisplayId?: string | null;
	providerId: number;
	modelId: string;
	stream: boolean;
	ttft?: number | null;
	inputTokens?: number | null;
	inputCacheTokens: number;
	inputCacheRate: number;
	outputTokens?: number | null;
	outputTokensTime?: number | null;
	tps: number;
	startTime: number;
	endTime: number;
	requestTime: number;
	success: boolean;
	failReason?: string | null;
	totalTokens?: number | null;
	apiKeyName: string;
}

export interface RequestLogPage {
	items: RequestLogRow[];
	total: number;
	page: number;
	pageSize: number;
}

export interface RequestLogFilters {
	page: number;
	pageSize: number;
	vmId?: number;
	apiKey?: string;
	startTime?: number;
	endTime?: number;
	sortBy?: string;
	sortOrder?: "asc" | "desc";
}

export const requestLogKeys = {
	all: ["request-logs"] as const,
	list: (filters: RequestLogFilters) => ["request-logs", filters] as const,
};

function buildQuery(filters: RequestLogFilters): string {
	const params = new URLSearchParams();
	params.set("page", String(filters.page));
	params.set("pageSize", String(filters.pageSize));
	if (filters.vmId !== undefined) params.set("vmId", String(filters.vmId));
	if (filters.apiKey) params.set("apiKey", filters.apiKey);
	if (filters.startTime !== undefined) params.set("startTime", String(filters.startTime));
	if (filters.endTime !== undefined) params.set("endTime", String(filters.endTime));
	if (filters.sortBy) params.set("sortBy", filters.sortBy);
	if (filters.sortOrder) params.set("sortOrder", filters.sortOrder);
	const qs = params.toString();
	return qs ? `?${qs}` : "";
}

/** 服务端分页查询请求日志。 */
export function useRequestLogs(filters: RequestLogFilters) {
	return useQuery<RequestLogPage>({
		queryKey: requestLogKeys.list(filters),
		queryFn: async () => {
			const res = await api
				.get(`request-logs${buildQuery(filters)}`)
				.json<ApiResponse<RequestLogPage>>();
			return unwrap(res);
		},
		placeholderData: (prev) => prev,
	});
}
