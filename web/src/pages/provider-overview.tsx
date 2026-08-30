import { CallAnalysisCard, TokenAnalysisCard } from "@/components/analysis-cards";
import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useDashboardCharts } from "@/hooks/use-dashboard-stats";
import {
	type ProviderModelRankItem,
	type RaceSort,
	type RaceSortKey,
	useProviderModelRace,
} from "@/hooks/use-provider-model-race";
import { useProviderDetail } from "@/hooks/use-providers";
import { type RacePeriod, formatPeriodLabel } from "@/lib/race-period";
import { formatTokenCount } from "@/lib/utils";
import { ArrowDown, ArrowUp, Boxes } from "lucide-react";
import { useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";

/** 二级页三个图表区块的独立时间段状态。 */
interface ProviderOverviewWindows {
	call: RaceWindowState;
	token: RaceWindowState;
	race: RaceWindowState;
}

/** 从 URL query 解析初始时间段（缺省当天）；首页赛马行点击时携带。 */
function initialWindowFromUrl(searchParams: URLSearchParams): RaceWindowState {
	const period = (searchParams.get("period") as RacePeriod | "custom" | null) ?? "day";
	const offset = Number.parseInt(searchParams.get("offset") ?? "0", 10) || 0;
	const now = Date.now();
	const startTime = Number(searchParams.get("startTime")) || now - 3_600_000;
	const endTime = Number(searchParams.get("endTime")) || now;
	return {
		period,
		offset,
		customStart: startTime,
		customEnd: endTime,
		appliedCustom: period === "custom" ? { startTime, endTime } : null,
	};
}

/** 6 列指标定义（内部模型赛马表格）。 */
const COLUMNS: ReadonlyArray<{
	key: RaceSortKey;
	label: string;
	format: (v: number) => string;
	defaultDesc: boolean;
}> = [
	{ key: "totalTokens", label: "总计 Token", format: formatTokenCount, defaultDesc: true },
	{ key: "requestCount", label: "请求数", format: (v) => v.toLocaleString(), defaultDesc: true },
	{ key: "ttft", label: "TTFT", format: (v) => `${v.toFixed(1)} ms`, defaultDesc: false },
	{
		key: "requestTime",
		label: "平均耗时",
		format: (v) => `${v.toFixed(1)} ms`,
		defaultDesc: false,
	},
	{ key: "tps", label: "TPS", format: (v) => v.toFixed(2), defaultDesc: true },
	{
		key: "cacheHitRate",
		label: "缓存命中率",
		format: (v) => `${(v * 100).toFixed(1)}%`,
		defaultDesc: true,
	},
];

/** 供应商内部模型赛马表格（按供应商过滤 + 6 指标 + 排序）。 */
function InternalModelRaceTable({
	providerId,
	windowState,
	now,
}: {
	providerId: number;
	windowState: RaceWindowState;
	now: number;
}) {
	const [sort, setSort] = useState<RaceSort>({ sortBy: "totalTokens", sortOrder: "desc" });
	const window = raceWindowBounds(windowState, now);
	const query = useProviderModelRace(window, sort, true, providerId);

	const handleSort = (key: RaceSortKey) => {
		setSort((prev) => {
			if (prev.sortBy === key) {
				return { ...prev, sortOrder: prev.sortOrder === "asc" ? "desc" : "asc" };
			}
			const column = COLUMNS.find((c) => c.key === key);
			return { sortBy: key, sortOrder: column?.defaultDesc ? "desc" : "asc" };
		});
	};

	return (
		<div className="overflow-x-auto">
			<table className="w-full min-w-[720px] border-collapse text-sm">
				<thead>
					<tr className="border-b border-foreground/10">
						<th className="w-10 px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							#
						</th>
						<th className="px-2 py-2 text-left text-xs font-medium text-muted-foreground">模型</th>
						{COLUMNS.map((column) => {
							const active = sort.sortBy === column.key;
							return (
								<th key={column.key} className="px-2 py-2 text-right">
									<button
										type="button"
										onClick={() => handleSort(column.key)}
										aria-label={column.label}
										className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium transition-colors hover:bg-foreground/5 ${
											active ? "text-foreground" : "text-muted-foreground"
										}`}
									>
										{column.label}
										{active &&
											(sort.sortOrder === "asc" ? (
												<ArrowUp data-testid={`sort-${column.key}`} className="h-3 w-3" />
											) : (
												<ArrowDown data-testid={`sort-${column.key}`} className="h-3 w-3" />
											))}
									</button>
								</th>
							);
						})}
					</tr>
				</thead>
				<tbody>
					{query.data?.items.map((item: ProviderModelRankItem, index: number) => (
						<tr
							key={item.modelId}
							className="border-b border-foreground/5 last:border-0 hover:bg-foreground/5"
						>
							<td className="px-2 py-2 text-left font-mono text-xs text-muted-foreground">
								{index + 1}
							</td>
							<td className="px-2 py-2 text-left font-medium text-foreground">{item.modelId}</td>
							{COLUMNS.map((column) => (
								<td
									key={column.key}
									className="px-2 py-2 text-right font-mono text-xs tabular-nums text-foreground"
								>
									{column.format(item[column.key])}
								</td>
							))}
						</tr>
					))}
				</tbody>
			</table>
		</div>
	);
}

/** 供应商二级数据面板：调用分析 + token 分析 + 内部模型赛马，三块独立时间段。 */
export default function ProviderOverviewPage() {
	const { providerId: providerIdParam } = useParams();
	const providerId = Number.parseInt(providerIdParam ?? "", 10);
	const [searchParams] = useSearchParams();

	// 三块独立时间段，初始值来自 URL（无参数默认当天）。
	const [windows, setWindows] = useState<ProviderOverviewWindows>(() => {
		const initial = initialWindowFromUrl(searchParams);
		return { call: { ...initial }, token: { ...initial }, race: { ...initial } };
	});
	// 各块固化 now（标题稳定）。
	const [now] = useState(() => Date.now());

	const providerDetail = useProviderDetail(Number.isFinite(providerId) ? providerId : null);
	const providerName = providerDetail.data?.name ?? `供应商 #${providerId}`;

	const callWindow = raceWindowBounds(windows.call, now);
	const tokenWindow = raceWindowBounds(windows.token, now);

	const callCharts = useDashboardCharts({
		startTime: callWindow.startTime,
		endTime: callWindow.endTime,
		providerId,
	});
	const tokenCharts = useDashboardCharts({
		startTime: tokenWindow.startTime,
		endTime: tokenWindow.endTime,
		providerId,
	});

	const windowSubtitle = (state: RaceWindowState) =>
		state.period === "custom"
			? "自定义时间范围"
			: formatPeriodLabel(state.period, state.offset, now);

	return (
		<div className="space-y-6">
			<div className="flex items-center gap-2">
				<Boxes className="size-5 text-muted-foreground" />
				<h1 className="text-2xl font-bold tracking-tight">{providerName} · 数据面板</h1>
			</div>

			{/* 调用分析：独立时间段（CallAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div className="flex flex-wrap items-center justify-between gap-2">
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.call)}</p>
					<RaceWindowControl
						state={windows.call}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, call: { ...prev.call, ...patch } }))
						}
					/>
				</div>
				{callCharts.isLoading ? (
					<Skeleton className="h-[260px] w-full" />
				) : callCharts.isError || !callCharts.data ? (
					<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
						数据加载失败
					</div>
				) : (
					<CallAnalysisCard charts={callCharts.data} subtitle={windowSubtitle(windows.call)} />
				)}
			</div>

			{/* Token 分析：独立时间段（TokenAnalysisCard 自带卡片壳） */}
			<div className="space-y-2">
				<div className="flex flex-wrap items-center justify-between gap-2">
					<p className="text-xs text-muted-foreground">{windowSubtitle(windows.token)}</p>
					<RaceWindowControl
						state={windows.token}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, token: { ...prev.token, ...patch } }))
						}
					/>
				</div>
				{tokenCharts.isLoading ? (
					<Skeleton className="h-[260px] w-full" />
				) : tokenCharts.isError || !tokenCharts.data ? (
					<div className="flex h-[260px] items-center justify-center text-xs text-muted-foreground">
						数据加载失败
					</div>
				) : (
					<TokenAnalysisCard charts={tokenCharts.data} subtitle={windowSubtitle(windows.token)} />
				)}
			</div>

			{/* 供应商内部模型赛马：独立时间段 */}
			<Card>
				<CardHeader className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="space-y-1">
						<CardTitle>内部模型赛马</CardTitle>
						<p className="text-xs text-muted-foreground">{windowSubtitle(windows.race)}</p>
					</div>
					<RaceWindowControl
						state={windows.race}
						now={now}
						onChange={(patch) =>
							setWindows((prev) => ({ ...prev, race: { ...prev.race, ...patch } }))
						}
					/>
				</CardHeader>
				<CardContent>
					<InternalModelRaceTable providerId={providerId} windowState={windows.race} now={now} />
				</CardContent>
			</Card>
		</div>
	);
}
