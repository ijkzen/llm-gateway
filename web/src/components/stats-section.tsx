import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import { ErrorState } from "@/components/error-state";
import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import {
	RaceWindowControl,
	type RaceWindowState,
	initialWindowFromUrl,
	raceWindowBounds,
	useSectionSubtitle,
} from "@/components/race-window-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import type { InsightData } from "@/hooks/use-dashboard-insight";
import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import type { ChartGranularity } from "@/lib/race-period";
import { chartGranularity } from "@/lib/race-period";
import { type ReactNode, useState } from "react";
import { useSearchParams } from "react-router-dom";

/** 区块副标题：实现归位到 race-window-control.tsx（赛马卡壳共用），此处保留出口。 */
export { useSectionSubtitle };

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

/** 区块窗口（毫秒起止；timeZone 缺省按浏览器本地解释）。 */
export function sectionWindow(state: RaceWindowState, now: number, timeZone?: string) {
	return raceWindowBounds(state, now, timeZone);
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

/** 区块内容的三态分支（骨架/错误重试/内容），两种区块壳共用。 */
function SectionBody({ status, children }: { status?: SectionStatus; children: ReactNode }) {
	if (!status) {
		return <>{children}</>;
	}
	if (status.isLoading) {
		return <Skeleton className="h-[260px] w-full" />;
	}
	if (status.isError) {
		return <ErrorState onRetry={status.onRetry} />;
	}
	return <>{children}</>;
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
	return (
		<div className="space-y-2">
			<div className="flex flex-wrap items-center justify-between gap-2" data-testid={windowTestId}>
				<p className="text-xs text-muted-foreground">{subtitle}</p>
				<RaceWindowControl state={windowState} now={now} onChange={onWindowChange} />
			</div>
			<SectionBody status={status}>{children}</SectionBody>
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
	return (
		<Card>
			<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="space-y-1">
					<CardTitle>{title}</CardTitle>
					<p className="text-xs text-muted-foreground">{subtitle}</p>
				</div>
				<RaceWindowControl state={windowState} now={now} onChange={onWindowChange} />
			</CardHeader>
			<CardContent>
				<SectionBody status={status}>{children}</SectionBody>
			</CardContent>
		</Card>
	);
}

/** 分析区块的键（窗口 map 的键与粒度集合共用）。 */
export type AnalysisSectionKey = "call" | "token" | "insight";

/** 分析区块查询结果的最小结构（TanStack Query 结果子集，便于注入与测试）。 */
interface AnalysisQuery<T> {
	data: T | undefined;
	isLoading: boolean;
	isError: boolean;
	refetch: () => void;
}

export interface AnalysisSectionsProps {
	now: number;
	/** 三区块各自的独立时间窗。 */
	windows: Record<AnalysisSectionKey, RaceWindowState>;
	onWindowChange: (key: AnalysisSectionKey) => (patch: Partial<RaceWindowState>) => void;
	/** 各区块由窗口推导的桶粒度。 */
	granularities: Record<AnalysisSectionKey, ChartGranularity>;
	callCharts: AnalysisQuery<DashboardCharts>;
	tokenCharts: AnalysisQuery<DashboardCharts>;
	insight: AnalysisQuery<InsightData>;
}

/**
 * 「调用分析 / Token 分析 / 性能与可靠性」三区块装配（16-11）：API Key、
 * 虚拟模型、供应商三个二级页此前各 ~60 行逐字重复，差异只有查询对象、窗口
 * 与粒度（页面级门控由调用方保留，模型页的折线变体不走本组件）。
 */
export function AnalysisSections({
	now,
	windows,
	onWindowChange,
	granularities,
	callCharts,
	tokenCharts,
	insight,
}: AnalysisSectionsProps) {
	const subtitle = useSectionSubtitle();
	return (
		<>
			<StatsSection
				now={now}
				windowState={windows.call}
				onWindowChange={onWindowChange("call")}
				status={{
					isLoading: callCharts.isLoading,
					isError: callCharts.isError || !callCharts.data,
					onRetry: () => callCharts.refetch(),
				}}
			>
				{callCharts.data && (
					<CallAnalysisCard
						charts={callCharts.data}
						subtitle={subtitle(windows.call, now)}
						granularity={granularities.call}
					/>
				)}
			</StatsSection>

			<StatsSection
				now={now}
				windowState={windows.token}
				onWindowChange={onWindowChange("token")}
				status={{
					isLoading: tokenCharts.isLoading,
					isError: tokenCharts.isError || !tokenCharts.data,
					onRetry: () => tokenCharts.refetch(),
				}}
			>
				{tokenCharts.data && (
					<TokenAnalysisCard
						charts={tokenCharts.data}
						subtitle={subtitle(windows.token, now)}
						granularity={granularities.token}
					/>
				)}
			</StatsSection>

			<StatsSection
				now={now}
				windowState={windows.insight}
				onWindowChange={onWindowChange("insight")}
				status={{
					isLoading: insight.isLoading,
					isError: insight.isError || !insight.data,
					onRetry: () => insight.refetch(),
				}}
			>
				{insight.data && (
					<InsightAnalysisCard
						data={insight.data}
						subtitle={subtitle(windows.insight, now)}
						granularity={granularities.insight}
					/>
				)}
			</StatsSection>
		</>
	);
}
