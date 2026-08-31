import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

export const LOCALES = ["zh-CN", "en"] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_STORAGE_KEY = "llm-gateway-locale";

/** 语言设置项 key（与后端设置表一致）。 */
export const SETTING_KEY_LANGUAGE = "language";
/** 时区设置项 key（与后端设置表一致）。 */
export const SETTING_KEY_TIMEZONE = "timezone";

/** 从浏览器语言推断支持的语言（zh→zh-CN，其余非中文→en）。 */
export function detectBrowserLocale(): Locale {
	const lang = navigator.language.toLowerCase();
	if (lang.startsWith("zh")) {
		return "zh-CN";
	}
	return "en";
}

/** 从 localStorage（zustand persist 写入）读取已保存的语言。 */
export function storedLocale(): Locale | null {
	try {
		const raw = localStorage.getItem(LOCALE_STORAGE_KEY);
		if (!raw) return null;
		const parsed = JSON.parse(raw) as { state?: { locale?: string } };
		const value = parsed.state?.locale;
		if (value === "zh-CN" || value === "en") {
			return value;
		}
		return null;
	} catch {
		return null;
	}
}

/** 初始语言：localStorage → 浏览器语言 → zh-CN。 */
export function initialLocale(): Locale {
	return storedLocale() ?? detectBrowserLocale();
}

void i18n.use(initReactI18next).init({
	resources: {
		"zh-CN": { translation: zhCN },
		en: { translation: en },
	},
	lng: initialLocale(),
	fallbackLng: "zh-CN",
	interpolation: {
		escapeValue: false,
	},
});

// 同步 <html lang>，让浏览器原生 UI（日期选择器等）跟随语言。
i18n.on("languageChanged", (lng) => {
	document.documentElement.lang = lng;
});

export default i18n;
