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
	title: string;
	description: string;
	icon: LucideIcon;
}

export const OVERVIEW_PAGE: PageConfig = {
	path: "/",
	title: "数据面板",
	description: "请求指标概览与过去 24 小时调用/token 图表",
	icon: LayoutDashboard,
};

export const CRON_JOBS_PAGE: PageConfig = {
	path: "/cron-jobs",
	title: "定时任务",
	description: "管理系统定时任务，支持启用/禁用、立即执行与编辑",
	icon: Clock,
};

export const SETTINGS_PAGE: PageConfig = {
	path: "/settings",
	title: "系统设置",
	description: "管理应用配置项",
	icon: Settings,
};

export const PROVIDERS_PAGE: PageConfig = {
	path: "/providers",
	title: "供应商",
	description: "管理供应商接入实例，支持创建、编辑与删除",
	icon: Server,
};

export const PROVIDER_MODELS_PAGE: PageConfig = {
	path: "/provider-models",
	title: "供应商模型",
	description: "管理各供应商名下的模型清单与能力，支持刷新智能填充",
	icon: Boxes,
};

export const VIRTUAL_MODELS_PAGE: PageConfig = {
	path: "/virtual-models",
	title: "虚拟模型",
	description: "将多个供应商模型聚合为对外统一的虚拟模型",
	icon: Layers,
};

export const API_KEYS_PAGE: PageConfig = {
	path: "/api-keys",
	title: "API Key",
	description: "管理调用方访问网关的 API Key，支持创建、禁用与删除",
	icon: KeyRound,
};

export const REQUEST_LOGS_PAGE: PageConfig = {
	path: "/request-logs",
	title: "请求日志",
	description: "查看网关转发的请求日志，支持时间段/虚拟模型/API Key 过滤",
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
