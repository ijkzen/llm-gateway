import type { LucideIcon } from "lucide-react";
import { Clock, LayoutDashboard, Server, Settings } from "lucide-react";

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
	title: "模型提供商",
	description: "管理模型提供商接入实例，支持创建、编辑与删除",
	icon: Server,
};

export const PAGES: readonly PageConfig[] = [
	OVERVIEW_PAGE,
	CRON_JOBS_PAGE,
	PROVIDERS_PAGE,
	SETTINGS_PAGE,
];
