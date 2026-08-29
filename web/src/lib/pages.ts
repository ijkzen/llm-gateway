import type { LucideIcon } from "lucide-react";
import { Boxes, Clock, Layers, LayoutDashboard, Server, Settings } from "lucide-react";

export interface PageConfig {
	path: string;
	title: string;
	description: string;
	icon: LucideIcon;
}

export const OVERVIEW_PAGE: PageConfig = {
	path: "/",
	title: "总览",
	description: "系统概览与最近动态",
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

export const PAGES: readonly PageConfig[] = [
	OVERVIEW_PAGE,
	CRON_JOBS_PAGE,
	PROVIDERS_PAGE,
	PROVIDER_MODELS_PAGE,
	VIRTUAL_MODELS_PAGE,
	SETTINGS_PAGE,
];
