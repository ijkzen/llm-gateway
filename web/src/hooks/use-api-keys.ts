import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

export interface ApiKey {
	id: number;
	name: string;
	/** 掩码后的 key，如 `lg-****abcd`；明文仅详情接口返回。 */
	keyMasked: string;
	enable: boolean;
	createdAt: string;
	updatedAt: string;
}

/** 详情接口额外返回明文 key（前端通过小眼睛切换展示）。 */
export interface ApiKeyDetail extends ApiKey {
	key: string;
}

export const apiKeyKeys = {
	all: ["api-keys"] as const,
	detail: (id: number) => ["api-keys", id] as const,
};

/** 拉取单个 API Key 详情（含明文 key），供 query 与命令式复制共用。 */
export async function fetchApiKeyDetail(id: number): Promise<ApiKeyDetail> {
	const res = await api.get(`api-keys/${id}`).json<ApiResponse<ApiKeyDetail>>();
	return unwrap(res);
}

export function useApiKeys() {
	return useQuery<ApiKey[]>({
		queryKey: apiKeyKeys.all,
		queryFn: async () => {
			const res = await api.get("api-keys").json<ApiResponse<ApiKey[]>>();
			return unwrap(res);
		},
	});
}

export function useApiKeyDetail(id: number | null) {
	return useQuery<ApiKeyDetail>({
		queryKey: apiKeyKeys.detail(id ?? -1),
		queryFn: () => fetchApiKeyDetail(id as number),
		enabled: id !== null,
	});
}

export function useCreateApiKey() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: { name: string }) => {
			const res = await api.post("api-keys", { json: payload }).json<ApiResponse<ApiKey>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: apiKeyKeys.all }),
	});
}

export function useToggleApiKey() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: { id: number; enable: boolean }) => {
			const res = await api
				.put(`api-keys/${payload.id}`, { json: { enable: payload.enable } })
				.json<ApiResponse<ApiKey>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: apiKeyKeys.all }),
	});
}

export function useDeleteApiKey() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (id: number) => {
			const res = await api.delete(`api-keys/${id}`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: apiKeyKeys.all }),
	});
}
