import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

export interface Provider {
	id: number;
	name: string;
	enable: boolean;
	baseUrl: string;
	apiKeyMasked: string;
	status: number;
	protocolType: number;
	billingMode: number;
	customHeader: string;
	extra: string;
	proxyEnabled: boolean;
	proxyAddr: string;
	createdAt: string;
	updatedAt: string;
}

/**
 * 按需获取 Provider 明文 API Key（详情接口不返回明文）。
 * 仅命令式调用，不经过 React Query 缓存，每次点击「显示/复制」都重新请求。
 */
export async function fetchProviderApiKey(id: number): Promise<string> {
	const res = await api.get(`providers/${id}/api-key`).json<ApiResponse<{ apiKey: string }>>();
	return (await unwrap(res)).apiKey;
}

export interface ProviderTemplate {
	name: string;
	baseUrl: string;
	protocolType: number;
	billingMode: number;
	extra: string;
}

export interface ProviderPayload {
	name: string;
	enable: boolean;
	baseUrl: string;
	apiKey: string;
	protocolType: number;
	billingMode: number;
	customHeader: string;
	extra: string;
	proxyEnabled: boolean;
	proxyAddr: string;
}

export interface UpdateProviderPayload {
	name?: string;
	enable?: boolean;
	baseUrl?: string;
	apiKey?: string;
	protocolType?: number;
	billingMode?: number;
	customHeader?: string;
	extra?: string;
	proxyEnabled?: boolean;
	proxyAddr?: string;
}

export const providerKeys = {
	all: ["providers"] as const,
	detail: (id: number) => ["providers", id] as const,
	templateMatch: ["provider-templates", "match"] as const,
};

export function useProviders() {
	return useQuery<Provider[]>({
		queryKey: providerKeys.all,
		queryFn: async () => {
			const res = await api.get("providers").json<ApiResponse<Provider[]>>();
			return unwrap(res);
		},
	});
}

export function useProviderDetail(id: number | null) {
	return useQuery<Provider>({
		queryKey: providerKeys.detail(id ?? -1),
		queryFn: async () => {
			const res = await api.get(`providers/${id}`).json<ApiResponse<Provider>>();
			return unwrap(res);
		},
		enabled: id !== null,
	});
}

/** 按 Base URL 匹配模板，返回全部命中（同一 host 可能有多个模板）；未命中返回空数组（后端 404 在此吞掉）。 */
export function useMatchTemplate(baseUrl: string) {
	return useQuery<ProviderTemplate[]>({
		queryKey: [...providerKeys.templateMatch, baseUrl],
		queryFn: async () => {
			const res = await api
				.post("provider-templates/match", { json: { baseUrl } })
				.json<ApiResponse<ProviderTemplate[]>>();
			try {
				return unwrap(res);
			} catch {
				return [];
			}
		},
		enabled: baseUrl.trim().length > 0,
	});
}

export function useCreateProvider() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: ProviderPayload) => {
			const res = await api.post("providers", { json: payload }).json<ApiResponse<Provider>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerKeys.all }),
	});
}

export function useUpdateProvider() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: { id: number } & UpdateProviderPayload) => {
			const { id, ...body } = payload;
			const res = await api.put(`providers/${id}`, { json: body }).json<ApiResponse<Provider>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerKeys.all }),
	});
}

/** 批量重排供应商列表顺序（按 ids 数组顺序），成功后刷新列表。 */
export function useReorderProviders() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (ids: number[]) => {
			const res = await api
				.put("providers/reorder", { json: { ids } })
				.json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		// 成功/失败都重新拉取服务端数据：成功确认新顺序，失败则回滚乐观更新。
		onSettled: () => queryClient.invalidateQueries({ queryKey: providerKeys.all }),
	});
}

export function useDeleteProvider() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (id: number) => {
			const res = await api.delete(`providers/${id}`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerKeys.all }),
	});
}
