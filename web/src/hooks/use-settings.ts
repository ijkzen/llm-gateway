import { useLocale } from "@/hooks/use-locale";
import i18n, { SETTING_KEY_LANGUAGE } from "@/i18n";
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
		onSuccess: async (_data, variables) => {
			await queryClient.invalidateQueries({ queryKey: settingsKeys.all });
			// 18-03：设置表直视编辑 language 时前端同步热切换（正路 useChangeLocale
			// 之外的第二入口）——否则界面语言与后端设置表长期分叉，且下次刷新又
			// 从 localStorage 读回旧语言。
			if (variables.key !== SETTING_KEY_LANGUAGE) return;
			if (variables.value !== "zh-CN" && variables.value !== "en") return;
			const { setLocale } = useLocale.getState();
			setLocale(variables.value);
			if (i18n.language !== variables.value) {
				await i18n.changeLanguage(variables.value);
			}
			await queryClient.invalidateQueries();
		},
	});
}

export function useDeleteSetting() {
	const queryClient = useQueryClient();
	return useMutation({
		mutationFn: async (key: string) => {
			const res = await api.delete(`settings/${key}`).json<ApiResponse<unknown>>();
			return unwrap(res);
		},
		onSuccess: () => queryClient.invalidateQueries({ queryKey: settingsKeys.all }),
	});
}
