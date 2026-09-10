import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useQuery } from "@tanstack/react-query";

/** 请求日志行（与后端 GET /api/request-logs 的 items 对齐）。 */
export interface RequestLogRow {
	requestId: string;
	virtualModelId: number;
	virtualModelDisplayId?: string | null;
	providerId: number;
	/** 供应商名称（后端 LEFT JOIN provider 补出；供应商缺失时为 null，兜底显示 #providerId）。 */
	providerName?: string | null;
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
	/** 多值过滤：空数组/缺省 = 不过滤（前端勾满全部选项时归一化为空数组）。 */
	vmId?: number[];
	/** 按供应商过滤（request.provider_id），多值。 */
	providerId?: number[];
	/** 按供应商模型过滤（request.model_id，供应商侧真实模型 ID），多值。 */
	modelId?: string[];
	/** 按结果状态过滤：省略 = 全部，true = 成功，false = 失败。 */
	success?: boolean;
	apiKey?: string[];
	startTime?: number;
	endTime?: number;
	sortBy?: string;
	sortOrder?: "asc" | "desc";
}

export const requestLogKeys = {
	all: ["request-logs"] as const,
	list: (filters: RequestLogFilters) => ["request-logs", filters] as const,
};

/**
 * 逗号是后端 CSV 过滤参数的分隔符（`split(',')`），而 API Key 名允许含逗号
 * （后端仅校验非空）。名字里带逗号时会被拆成两段、静默扩大过滤范围，故此处
 * 剔除含分隔符的值（16-13：约束在生成侧显式化；后端侧同族问题见 11-16）。
 */
function csvValue(values: string[] | undefined): string | undefined {
	const cleaned = values?.filter((v) => !v.includes(","));
	return cleaned?.length ? cleaned.join(",") : undefined;
}

function buildQuery(filters: RequestLogFilters): string {
	const params = new URLSearchParams();
	params.set("page", String(filters.page));
	params.set("pageSize", String(filters.pageSize));
	if (filters.vmId?.length) params.set("vmId", filters.vmId.join(","));
	if (filters.providerId?.length) params.set("providerId", filters.providerId.join(","));
	if (filters.modelId?.length) params.set("modelId", filters.modelId.join(","));
	if (filters.success !== undefined) params.set("success", String(filters.success));
	const apiKeyCsv = csvValue(filters.apiKey);
	if (apiKeyCsv) params.set("apiKey", apiKeyCsv);
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
