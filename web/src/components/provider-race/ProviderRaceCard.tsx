import { MidEllipsis } from "@/components/mid-ellipsis";
import {
	RaceWindowControl,
	type RaceWindowState,
	defaultRaceWindowState,
	raceWindowBounds,
	windowQueryString,
} from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useInView } from "@/hooks/use-in-view";
import { type ProviderRankItem, useProviderRace } from "@/hooks/use-provider-race";
import { formatPeriodLabel } from "@/lib/race-period";
import { TrendingUp } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

/**
 * 供应商赛马卡片：单卡片聚合展示 6 个指标（总计 Token / 请求数 / TTFT /
 * 平均耗时 / TPS / 缓存命中率），可点表头按任意指标升/降序；时间窗口支持
 * 天/周/月/年（左右箭头切换周期）+ 自定义（秒级）。卡片进入视口才发请求。
 * 点击行进入该供应商的二级数据面板页（携带当前时间段参数）。
 * 可选按调用方 API Key 过滤（API Key 数据面板「该 key 用到的供应商」）；
 * 可选初始时间窗（未传默认当天，供数据面板接收 URL 参数）。
 */
export function ProviderRaceCard({
	apiKey,
	initialWindow,
}: {
	apiKey?: string;
	initialWindow?: RaceWindowState;
}) {
	const navigate = useNavigate();
	const { t } = useTranslation();
	// 挂载时刻固化 now：保证「当前周期」的窗口终点稳定，不因渲染抖动重复请求。
	const [now] = useState(() => Date.now());
	const [windowState, setWindowState] = useState<RaceWindowState>(
		() => initialWindow ?? defaultRaceWindowState(),
	);

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const { sort, onSort } = useRaceSort();

	const window = raceWindowBounds(windowState, now);

	const { ref, inView } = useInView();
	const query = useProviderRace(window, sort, inView, apiKey);

	const openProviderOverview = (item: ProviderRankItem) => {
		navigate(`/providers/${item.providerId}/overview?${windowQueryString(windowState, window)}`);
	};

	return (
		<Card ref={ref} className="p-5">
			<div className="mb-4 flex flex-wrap items-center gap-3">
				<span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
					<TrendingUp className="h-4 w-4" />
				</span>
				<div className="min-w-0">
					<h3 className="text-sm font-semibold text-foreground">{t("dashboard.providerRace")}</h3>
					<MidEllipsis
						className="text-xs text-muted-foreground"
						text={
							windowState.period === "custom"
								? t("overview.customWindow")
								: formatPeriodLabel(windowState.period, windowState.offset, now)
						}
					/>
				</div>

				<div className="ml-auto">
					<RaceWindowControl
						state={windowState}
						now={now}
						onChange={(patch) => setWindowState((prev) => ({ ...prev, ...patch }))}
					/>
				</div>
			</div>

			{!inView ? (
				<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
					{t("race.loadingAfterScroll")}
				</div>
			) : query.isLoading ? (
				<Skeleton className="h-[220px] rounded-lg" />
			) : query.isError ? (
				<div className="flex h-[220px] flex-col items-center justify-center gap-2 text-xs text-muted-foreground">
					<span>{t("race.loadFailed")}</span>
					<button
						type="button"
						className="rounded-full bg-foreground/5 px-3 py-1 text-xs font-medium hover:bg-foreground/10"
						onClick={() => query.refetch()}
					>
						{t("common.retry")}
					</button>
				</div>
			) : !query.data || query.data.items.length === 0 ? (
				<div className="flex h-[220px] items-center justify-center text-xs text-muted-foreground">
					{t("race.noData")}
				</div>
			) : (
				<SortableMetricTable
					items={query.data.items}
					sort={sort}
					onSort={onSort}
					nameHeader="dashboard.providerColumn"
					renderName={(item) => item.providerName || t("race.unknownProvider")}
					rowKey={(item) => item.providerId}
					onRowClick={openProviderOverview}
					rowTitleKey="race.openProviderOverview"
				/>
			)}
		</Card>
	);
}
