import { type ApiResponse, api, unwrap } from "@/lib/api";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

/** 统一参数序列化：undefined 的键跳过，值统一转字符串。 */
export function statsSearchParams(params: Record<string, string | number | undefined>): string {
	const query = new URLSearchParams();
	for (const [key, value] of Object.entries(params)) {
		if (value !== undefined) {
			query.set(key, String(value));
		}
	}
	return query.toString();
}

/** 查询 key 段：面板共享 key 前缀（"stats" + 端点名）+ 稳定化片段（undefined→null）。 */
export function statsKey(
	endpoint: string,
	parts: ReadonlyArray<string | number | null | undefined>,
) {
	return ["stats", endpoint, ...parts.map((p) => p ?? null)] as const;
}

/** 过滤维度 key 段（rank/charts/insight 共用顺序：providerId/virtualModelId/modelId/apiKey）。 */
export function statsFilterKeySegments(filter: {
	providerId?: number;
	virtualModelId?: number;
	modelId?: string;
	apiKey?: string;
}): (string | number | null)[] {
	return [
		filter.providerId ?? null,
		filter.virtualModelId ?? null,
		filter.modelId ?? null,
		filter.apiKey ?? null,
	];
}

interface StatsQueryOptions {
	/** 相对 API 路径（如 "stats/provider-rank"）。 */
	endpoint: string;
	params: Record<string, string | number | undefined>;
	key: readonly unknown[];
	enabled?: boolean;
	/** 切换时间窗口期间保留上一窗口数据（默认 true，避免骨架闪回抖动）。 */
	keepPrevious?: boolean;
}

/** 面板数据查询内部收敛点：参数序列化、key、keepPreviousData 单处实现。 */
export function statsQuery<T>({
	endpoint,
	params,
	key,
	enabled = true,
	keepPrevious = true,
}: StatsQueryOptions) {
	return useQuery<T>({
		queryKey: key,
		queryFn: async () => {
			const qs = statsSearchParams(params);
			const suffix = qs ? `?${qs}` : "";
			const res = await api.get(`${endpoint}${suffix}`).json<ApiResponse<T>>();
			return unwrap(res);
		},
		enabled,
		placeholderData: keepPrevious ? keepPreviousData : undefined,
	});
}
