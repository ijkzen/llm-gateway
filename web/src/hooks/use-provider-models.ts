import { virtualModelKeys } from "@/hooks/use-virtual-models";
import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

/** 供应商模型：登记在供应商名下的具体模型条目。 */
export interface ProviderModel {
	modelId: number;
	providerId: number;
	providerModelId: string;
	contextLength: number;
	maxOutputTokens: number;
	reasoning: boolean;
	toolUse: boolean;
	imageUnderstand: boolean;
	videoUnderstand: boolean;
	createdAt: string;
	updatedAt: string;
}

/** 刷新候选的三态：smart=已智能填充、partial=信息不完整、manual=需手动填写。 */
export type MatchState = "smart" | "partial" | "manual";

export interface RefreshCandidate {
	providerModelId: string;
	matchState: MatchState;
	contextLength: number | null;
	maxOutputTokens: number | null;
	reasoning: boolean;
	toolUse: boolean;
	imageUnderstand: boolean;
	videoUnderstand: boolean;
}

export interface ProviderModelPayload {
	providerModelId: string;
	contextLength: number;
	maxOutputTokens: number;
	reasoning: boolean;
	toolUse: boolean;
	imageUnderstand: boolean;
	videoUnderstand: boolean;
}

export const providerModelKeys = {
	all: ["provider-models"] as const,
	catalogSearch: (q: string) => ["provider-models", "catalog-search", q] as const,
};

/** 模型目录关键词搜索候选（手动添加时的联想下拉）。 */
export interface CatalogCandidate {
	id: string;
	name: string;
	family: string;
	contextLength: number | null;
	maxOutputTokens: number | null;
	reasoning: boolean;
	toolUse: boolean;
	imageUnderstand: boolean;
	videoUnderstand: boolean;
}

/** 搜索内嵌模型目录；关键词为空时禁用。 */
export function useCatalogSearch(q: string) {
	const query = q.trim();
	return useQuery<CatalogCandidate[]>({
		queryKey: providerModelKeys.catalogSearch(query),
		queryFn: async () => {
			const res = await api
				.get(`provider-models/catalog/search?q=${encodeURIComponent(query)}&limit=8`)
				.json<ApiResponse<CatalogCandidate[]>>();
			return unwrap(res);
		},
		enabled: query.length > 0,
		placeholderData: (prev) => prev,
	});
}

export function useProviderModels() {
	return useQuery<ProviderModel[]>({
		queryKey: providerModelKeys.all,
		queryFn: async () => {
			const res = await api.get("provider-models").json<ApiResponse<ProviderModel[]>>();
			return unwrap(res);
		},
	});
}

export function useCreateProviderModel(providerId: number) {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: ProviderModelPayload) => {
			const res = await api
				.post(`providers/${providerId}/models`, { json: payload })
				.json<ApiResponse<ProviderModel>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerModelKeys.all }),
	});
}

export function useBatchCreateProviderModels(providerId: number) {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: { models: ProviderModelPayload[] }) => {
			const res = await api
				.post(`providers/${providerId}/models/batch`, { json: payload })
				.json<ApiResponse<ProviderModel[]>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerModelKeys.all }),
	});
}

export function useUpdateProviderModel(providerId: number) {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: ProviderModelPayload & { modelId: number }) => {
			const { modelId, ...body } = payload;
			const res = await api
				.put(`providers/${providerId}/models/${modelId}`, { json: body })
				.json<ApiResponse<ProviderModel>>();
			return unwrap(res);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: providerModelKeys.all });
			// 模型字段变化会反映在虚拟模型成员（能力图标等）上，一并刷新。
			queryClient.invalidateQueries({ queryKey: virtualModelKeys.all });
		},
	});
}

export function useDeleteProviderModel(providerId: number) {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (modelId: number) => {
			const res = await api
				.delete(`providers/${providerId}/models/${modelId}`)
				.json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => {
			queryClient.invalidateQueries({ queryKey: providerModelKeys.all });
			// 删除模型会级联清理引用它的虚拟模型成员，一并刷新。
			queryClient.invalidateQueries({ queryKey: virtualModelKeys.all });
		},
	});
}

/** 尝试刷新：调远端 Models 接口 + 服务端智能填充；远端可能较慢，单独放宽超时。 */
export function useRefreshProviderModels(providerId: number) {
	return useMutation({
		mutationFn: async (): Promise<RefreshCandidate[]> => {
			const res = await api
				.post(`providers/${providerId}/models/refresh`, { timeout: 30000 })
				.json<ApiResponse<RefreshCandidate[]>>();
			return unwrap(res);
		},
	});
}

/** 测试模型有效性：后端构建最小化请求发往上游；可能较慢（含建连/上游处理），单独放宽超时。成功返回本次请求耗时（ms）。 */
export function useTestProviderModel(providerId: number) {
	return useMutation({
		mutationFn: async (modelId: number): Promise<number> => {
			const res = await api
				.post(`providers/${providerId}/models/${modelId}/test`, { timeout: 60000 })
				.json<ApiResponse<{ duration_ms: number }>>();
			const data = await unwrap(res);
			return data.duration_ms;
		},
	});
}
