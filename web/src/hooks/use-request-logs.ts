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
	/** 按供应商过滤（request.provider_id）。 */
	providerId?: number;
	/** 按供应商模型过滤（request.model_id，供应商侧真实模型 ID）。 */
	modelId?: string;
	/** 按结果状态过滤：省略 = 全部，true = 成功，false = 失败。 */
	success?: boolean;
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
	if (filters.providerId !== undefined) params.set("providerId", String(filters.providerId));
	if (filters.modelId) params.set("modelId", filters.modelId);
	if (filters.success !== undefined) params.set("success", String(filters.success));
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
