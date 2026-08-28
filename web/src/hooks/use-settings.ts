import { type ApiResponse, api, unwrap } from "@/lib/api";
import type { SettingType } from "@/lib/constants";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

export interface Setting {
	key: string;
	value: string;
	type: SettingType;
	updated_at: string;
}

export const settingsKeys = {
	all: ["settings"] as const,
};

export function useSettings() {
	return useQuery<Setting[]>({
		queryKey: settingsKeys.all,
		queryFn: async () => {
			const res = await api.get("settings").json<ApiResponse<Setting[]>>();
			return unwrap(res);
		},
	});
}

export function useUpdateSetting() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async ({ key, value }: { key: string; value: string }) => {
			const res = await api
				.put(`settings/${key}`, { json: { value } })
				.json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: settingsKeys.all }),
	});
}
