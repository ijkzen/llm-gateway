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
	createdAt: string;
	updatedAt: string;
}

/** 详情接口额外返回明文 apiKey（前端通过小眼睛切换展示）。 */
export interface ProviderDetail extends Provider {
	apiKey: string;
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
	return useQuery<ProviderDetail>({
		queryKey: providerKeys.detail(id ?? -1),
		queryFn: async () => {
			const res = await api.get(`providers/${id}`).json<ApiResponse<ProviderDetail>>();
			return unwrap(res);
		},
		enabled: id !== null,
	});
}

/** 按 Base URL 匹配模板；未命中时返回 null（后端 404 在此吞掉）。 */
export function useMatchTemplate(baseUrl: string) {
	return useQuery<ProviderTemplate | null>({
		queryKey: [...providerKeys.templateMatch, baseUrl],
		queryFn: async () => {
			const res = await api
				.post("provider-templates/match", { json: { baseUrl } })
				.json<ApiResponse<ProviderTemplate | null>>();
			try {
				return unwrap(res);
			} catch {
				return null;
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
