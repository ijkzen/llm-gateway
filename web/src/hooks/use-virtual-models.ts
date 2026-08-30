import { type ApiResponse, api, unwrap } from "@/lib/api";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

/** 虚拟模型成员：指向一个供应商模型条目，附带供应商展示信息。 */
export interface VirtualModelItem {
	virtualModelItemId: number;
	modelId: number;
	enable: boolean;
	providerId: number;
	providerName: string;
	/** 供应商启用状态；false 时该成员实际不可用。 */
	providerEnable: boolean;
	/** 供应商付费模式：0=按量付费，1=订阅制。 */
	billingMode: number;
	/** 远端模型 ID 字符串，如 `gpt-4o`。 */
	providerModelId: string;
	contextLength: number;
	maxOutputTokens: number;
	reasoning: boolean;
	toolUse: boolean;
	imageUnderstand: boolean;
	videoUnderstand: boolean;
}

/** 虚拟模型：对外暴露的模型，聚合多个供应商模型。 */
export interface VirtualModel {
	virtualModelId: number;
	displayId: string;
	enable: boolean;
	loadBalancingStrategy: number;
	fallbackStrategy: number;
	items: VirtualModelItem[];
	createdAt: string;
	updatedAt: string;
}

export interface VirtualModelItemPayload {
	modelId: number;
	enable?: boolean;
}

export interface VirtualModelPayload {
	displayId: string;
	enable?: boolean;
	loadBalancingStrategy: number;
	fallbackStrategy: number;
	items?: VirtualModelItemPayload[];
}

/** 更新负载与创建共用同一后端端点，字段均可选（缺省表示不修改）。 */
export interface UpdateVirtualModelPayload {
	displayId?: string;
	enable?: boolean;
	loadBalancingStrategy?: number;
	fallbackStrategy?: number;
	items?: VirtualModelItemPayload[];
}

export const virtualModelKeys = {
	all: ["virtual-models"] as const,
	detail: (id: number) => ["virtual-models", id] as const,
};

export function useVirtualModels() {
	return useQuery<VirtualModel[]>({
		queryKey: virtualModelKeys.all,
		queryFn: async () => {
			const res = await api.get("virtual-models").json<ApiResponse<VirtualModel[]>>();
			return unwrap(res);
		},
	});
}

/** 虚拟模型详情（二级数据面板页头用）。 */
export function useVirtualModelDetail(id: number | null) {
	return useQuery<VirtualModel>({
		queryKey: virtualModelKeys.detail(id ?? -1),
		queryFn: async () => {
			const res = await api.get(`virtual-models/${id}`).json<ApiResponse<VirtualModel>>();
			return unwrap(res);
		},
		enabled: id !== null,
	});
}

export function useCreateVirtualModel() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: VirtualModelPayload) => {
			const res = await api
				.post("virtual-models", { json: payload })
				.json<ApiResponse<VirtualModel>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: virtualModelKeys.all }),
	});
}

export function useUpdateVirtualModel() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (payload: UpdateVirtualModelPayload & { id: number }) => {
			const { id, ...body } = payload;
			const res = await api
				.put(`virtual-models/${id}`, { json: body })
				.json<ApiResponse<VirtualModel>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: virtualModelKeys.all }),
	});
}

export function useDeleteVirtualModel() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (id: number) => {
			const res = await api.delete(`virtual-models/${id}`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: virtualModelKeys.all }),
	});
}
