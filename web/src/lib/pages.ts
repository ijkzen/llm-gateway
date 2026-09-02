import type { LucideIcon } from "lucide-react";
import {
	Boxes,
	Clock,
	KeyRound,
	Layers,
	LayoutDashboard,
	ScrollText,
	Server,
	Settings,
} from "lucide-react";

export interface PageConfig {
	path: string;
	/** 标题 i18n key（nav.pages.<page>.title），用于侧边栏与页面标题。 */
	titleKey: string;
	icon: LucideIcon;
}

/** 侧边栏分组：组标题 i18n key + 组内页面（顺序即展示顺序）。 */
export interface PageGroup {
	labelKey: string;
	pages: readonly PageConfig[];
}

export const OVERVIEW_PAGE: PageConfig = {
	path: "/",
	titleKey: "nav.pages.overview.title",
	icon: LayoutDashboard,
};

export const CRON_JOBS_PAGE: PageConfig = {
	path: "/cron-jobs",
	titleKey: "nav.pages.cronJobs.title",
	icon: Clock,
};

export const SETTINGS_PAGE: PageConfig = {
	path: "/settings",
	titleKey: "nav.pages.settings.title",
	icon: Settings,
};

export const PROVIDERS_PAGE: PageConfig = {
	path: "/providers",
	titleKey: "nav.pages.providers.title",
	icon: Server,
};

export const PROVIDER_MODELS_PAGE: PageConfig = {
	path: "/provider-models",
	titleKey: "nav.pages.providerModels.title",
	icon: Boxes,
};

export const VIRTUAL_MODELS_PAGE: PageConfig = {
	path: "/virtual-models",
	titleKey: "nav.pages.virtualModels.title",
	icon: Layers,
};

export const API_KEYS_PAGE: PageConfig = {
	path: "/api-keys",
	titleKey: "nav.pages.apiKeys.title",
	icon: KeyRound,
};

export const REQUEST_LOGS_PAGE: PageConfig = {
	path: "/request-logs",
	titleKey: "nav.pages.requestLogs.title",
	icon: ScrollText,
};

export const PAGES: readonly PageConfig[] = [
	OVERVIEW_PAGE,
	CRON_JOBS_PAGE,
	PROVIDERS_PAGE,
	PROVIDER_MODELS_PAGE,
	VIRTUAL_MODELS_PAGE,
	API_KEYS_PAGE,
	REQUEST_LOGS_PAGE,
	SETTINGS_PAGE,
];

/** 侧边栏导航分组：概览 / 接入配置 / 观测 / 管理。 */
export const NAV_GROUPS: readonly PageGroup[] = [
	{ labelKey: "nav.groups.overview", pages: [OVERVIEW_PAGE] },
	{
		labelKey: "nav.groups.access",
		pages: [PROVIDERS_PAGE, PROVIDER_MODELS_PAGE, VIRTUAL_MODELS_PAGE, API_KEYS_PAGE],
	},
	{ labelKey: "nav.groups.observability", pages: [REQUEST_LOGS_PAGE] },
	{ labelKey: "nav.groups.admin", pages: [CRON_JOBS_PAGE, SETTINGS_PAGE] },
];
