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
import { type ProviderModelRankItem, useProviderModelRace } from "@/hooks/use-provider-model-race";
import { formatPeriodLabel } from "@/lib/race-period";
import { Boxes } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

/** 名称列标签：供应商・模型（供应商缺失时退化为纯模型 ID）。 */
function modelLabel(item: Pick<ProviderModelRankItem, "providerName" | "modelId">): string {
	return item.providerName ? `${item.providerName}・${item.modelId}` : item.modelId;
}

/**
 * 供应商模型平铺赛马卡片：规格与供应商/虚拟模型赛马完全一致——单卡片表格，
 * 行的含义 = 供应商的每个模型（如 6 供应商 × 50 模型 = 300 行）；6 个指标
 * （总计 Token / 请求数 / TTFT / 平均耗时 / TPS / 缓存命中率），可点表头按
 * 任意指标升/降序；时间窗口天/周/月/年（左右箭头切换周期）+ 自定义（秒级）。
 * 卡片进入视口才发请求。
 * 可选按调用方 API Key 过滤（API Key 数据面板「该 key 用到的模型」）。
 */
export function ProviderModelRaceCard({
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
	const query = useProviderModelRace(window, sort, inView, undefined, apiKey);

	const openModelOverview = (item: ProviderModelRankItem) => {
		navigate(
			`/models/${item.providerId}/${encodeURIComponent(item.modelId)}/overview?${windowQueryString(windowState, window)}`,
		);
	};

	return (
		<Card ref={ref} className="p-5">
			<div className="mb-4 flex flex-wrap items-center gap-3">
				<span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
					<Boxes className="h-4 w-4" />
				</span>
				<div className="min-w-0">
					<h3 className="text-sm font-semibold text-foreground">
						{t("dashboard.providerModelRace")}
					</h3>
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
					nameHeader="dashboard.providerModel"
					renderName={modelLabel}
					rowKey={(item) => `${item.providerName}::${item.modelId}`}
					onRowClick={openModelOverview}
					rowTitleKey="race.openModelDetail"
				/>
			)}
		</Card>
	);
}
