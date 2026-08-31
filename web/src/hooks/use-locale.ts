import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { create } from "zustand";
import { persist } from "zustand/middleware";

import { LOCALE_STORAGE_KEY, type Locale, SETTING_KEY_LANGUAGE, initialLocale } from "@/i18n";
import { api } from "@/lib/api";

interface LocaleStore {
	locale: Locale;
	_hasHydrated: boolean;
	setHasHydrated: (v: boolean) => void;
	setLocale: (locale: Locale) => void;
}

export const useLocale = create<LocaleStore>()(
	persist(
		(set) => ({
			locale: initialLocale(),
			_hasHydrated: false,
			setHasHydrated: (hasHydrated) => set({ _hasHydrated: hasHydrated }),
			setLocale: (locale) => set({ locale }),
		}),
		{
			name: LOCALE_STORAGE_KEY,
			onRehydrateStorage: () => (state) => {
				state?.setHasHydrated(true);
			},
		},
	),
);

/**
 * 初始化语言：hydration 后把 i18n 切到持久化的语言（或首次访问时写入默认值）。
 * 在 App 根部与 useInitTheme 并列调用一次。
 */
export function useInitLocale() {
	const locale = useLocale((state) => state.locale);
	const hasHydrated = useLocale((state) => state._hasHydrated);
	const { i18n } = useTranslation();

	useEffect(() => {
		if (!hasHydrated) return;
		if (i18n.language !== locale) {
			void i18n.changeLanguage(locale);
		}
	}, [locale, hasHydrated, i18n]);
}

/**
 * 切换语言：更新本地 store（立即生效），并同步到后端设置表
 * （已登录时；失败不阻塞本地切换）。
 */
export function useChangeLocale() {
	const setLocale = useLocale((state) => state.setLocale);
	const { i18n } = useTranslation();

	return async (next: Locale) => {
		setLocale(next);
		await i18n.changeLanguage(next);
		try {
			await api.put(`settings/${SETTING_KEY_LANGUAGE}`, {
				json: { value: next },
			});
		} catch {
			// 未登录（401）或网络失败时静默：本地语言已切换，登录后会同步。
		}
	};
}
