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
};

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
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerModelKeys.all }),
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
		onSuccess: () => queryClient.invalidateQueries({ queryKey: providerModelKeys.all }),
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
