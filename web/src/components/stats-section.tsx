import { ErrorState } from "@/components/error-state";
import {
	RaceWindowControl,
	type RaceWindowState,
	initialWindowFromUrl,
	raceWindowBounds,
} from "@/components/race-window-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import type { ChartGranularity } from "@/lib/race-period";
import { chartGranularity, formatPeriodLabel } from "@/lib/race-period";
import { localeOf } from "@/lib/utils";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

/**
 * 多区块独立窗口状态：每页一份窗口 map（各区块初始值相同，来自 URL，
 * 无参数默认当天；可传 initial 工厂覆盖，如首页的「本地今日 0 点」）。
 */
export function useSectionWindows<const K extends string>(
	keys: readonly K[],
	initial?: () => RaceWindowState,
) {
	const [searchParams] = useSearchParams();
	const [windows, setWindows] = useState<Record<K, RaceWindowState>>(() => {
		const init = initial ? initial() : initialWindowFromUrl(searchParams);
		return Object.fromEntries(keys.map((k) => [k, { ...init }])) as Record<K, RaceWindowState>;
	});
	// 各块固化 now（标题/窗口终点稳定）。
	const [now] = useState(() => Date.now());
	const setWindow = (key: K) => (patch: Partial<RaceWindowState>) =>
		setWindows((prev) => ({ ...prev, [key]: { ...prev[key], ...patch } }));
	return { windows, now, setWindow };
}

/** 区块副标题：自定义区间文案 / 周期标签（各页逐字相同的闭包，收拢为一份）。 */
export function useSectionSubtitle() {
	const { t, i18n } = useTranslation();
	return (state: RaceWindowState, now: number) =>
		state.period === "custom"
			? t("overview.customWindow")
			: formatPeriodLabel(state.period, state.offset, now, localeOf(i18n.language));
}

/** 区块窗口（毫秒起止）。 */
export function sectionWindow(state: RaceWindowState, now: number) {
	return raceWindowBounds(state, now);
}

/** 图表桶粒度（由区块窗口推导）。 */
export function sectionGranularity(
	state: RaceWindowState,
	window: { startTime: number; endTime: number },
): ChartGranularity {
	return chartGranularity(state.period, window.startTime, window.endTime);
}

interface SectionStatus {
	isLoading: boolean;
	isError: boolean;
	onRetry: () => void;
}

interface StatsSectionProps {
	now: number;
	windowState: RaceWindowState;
	onWindowChange: (patch: Partial<RaceWindowState>) => void;
	/** 载入/错误状态；不传则直接渲染内容（页面级统一门控的场景，如首页）。 */
	status?: SectionStatus;
	/** 副标题行测试锚点（overview 页窗口行）。 */
	windowTestId?: string;
	children: ReactNode;
}

/**
 * 裸区块：副标题 + 窗口控件一行，内容区（分析卡片自带卡片壳）。
 * 数据面板五个页面的调用/Token/可靠性分析区块共用。
 */
export function StatsSection({
	now,
	windowState,
	onWindowChange,
	status,
	windowTestId,
	children,
}: StatsSectionProps) {
	const subtitle = useSectionSubtitle()(windowState, now);
	let body: ReactNode = children;
	if (status) {
		body = status.isLoading ? (
			<Skeleton className="h-[260px] w-full" />
		) : status.isError ? (
			<ErrorState onRetry={status.onRetry} />
		) : (
			children
		);
	}
	return (
		<div className="space-y-2">
			<div className="flex flex-wrap items-center justify-between gap-2" data-testid={windowTestId}>
				<p className="text-xs text-muted-foreground">{subtitle}</p>
				<RaceWindowControl state={windowState} now={now} onChange={onWindowChange} />
			</div>
			{body}
		</div>
	);
}

interface CardStatsSectionProps {
	title: string;
	now: number;
	windowState: RaceWindowState;
	onWindowChange: (patch: Partial<RaceWindowState>) => void;
	status?: SectionStatus;
	children: ReactNode;
}

/** 卡壳区块：CardTitle + 副标题 + 窗口控件的卡片头，内容区带载入/错误分支。 */
export function CardStatsSection({
	title,
	now,
	windowState,
	onWindowChange,
	status,
	children,
}: CardStatsSectionProps) {
	const subtitle = useSectionSubtitle()(windowState, now);
	let body: ReactNode = children;
	if (status) {
		body = status.isLoading ? (
			<Skeleton className="h-[260px] w-full" />
		) : status.isError ? (
			<ErrorState onRetry={status.onRetry} />
		) : (
			children
		);
	}
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{title}</CardTitle>
					<p className="text-xs text-muted-foreground">{subtitle}</p>
				</div>
				<RaceWindowControl state={windowState} now={now} onChange={onWindowChange} />
			</CardHeader>
			<CardContent>{body}</CardContent>
		</Card>
	);
}
