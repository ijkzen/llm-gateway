import {
	RaceWindowControl,
	type RaceWindowState,
	raceWindowBounds,
} from "@/components/race-window-control";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useInView } from "@/hooks/use-in-view";
import {
	type ProviderModelRankItem,
	type RaceSort,
	type RaceSortKey,
	useProviderModelRace,
} from "@/hooks/use-provider-model-race";
import { formatPeriodLabel } from "@/lib/race-period";
import { formatPercent, formatTokenCount } from "@/lib/utils";
import { ArrowDown, ArrowUp, Boxes } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

function initialWindowState(): RaceWindowState {
	return {
		period: "day",
		offset: 0,
		customStart: Date.now() - 3_600_000,
		customEnd: Date.now(),
		appliedCustom: null,
	};
}

/** 6 列指标定义：key / 标题键 / 格式化 / 默认方向（true=降序，耗时类默认升序）。 */
const COLUMNS: ReadonlyArray<{
	key: RaceSortKey;
	labelKey: string;
	format: (v: number) => string;
	defaultDesc: boolean;
}> = [
	{
		key: "totalTokens",
		labelKey: "race.metricLabel.totalTokens",
		format: formatTokenCount,
		defaultDesc: true,
	},
	{
		key: "requestCount",
		labelKey: "race.metricLabel.requestCount",
		format: (v) => v.toLocaleString(),
		defaultDesc: true,
	},
	{
		key: "ttft",
		labelKey: "race.metricLabel.ttft",
		format: (v) => `${v.toFixed(1)} ms`,
		defaultDesc: false,
	},
	{
		key: "requestTime",
		labelKey: "race.metricLabel.requestTime",
		format: (v) => `${v.toFixed(1)} ms`,
		defaultDesc: false,
	},
	{ key: "tps", labelKey: "race.metricLabel.tps", format: (v) => v.toFixed(2), defaultDesc: true },
	{
		key: "cacheHitRate",
		labelKey: "race.metricLabel.cacheHitRate",
		format: formatPercent,
		defaultDesc: true,
	},
];

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
 */
export function ProviderModelRaceCard() {
	const navigate = useNavigate();
	const { t } = useTranslation();
	// 挂载时刻固化 now：保证「当前周期」的窗口终点稳定，不因渲染抖动重复请求。
	const [now] = useState(() => Date.now());
	const [windowState, setWindowState] = useState<RaceWindowState>(initialWindowState);

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const [sort, setSort] = useState<RaceSort>({ sortBy: "totalTokens", sortOrder: "desc" });

	const window = raceWindowBounds(windowState, now);

	const { ref, inView } = useInView();
	const query = useProviderModelRace(window, sort, inView);

	const handleSort = (key: RaceSortKey) => {
		setSort((prev) => {
			if (prev.sortBy === key) {
				return { ...prev, sortOrder: prev.sortOrder === "asc" ? "desc" : "asc" };
			}
			const column = COLUMNS.find((c) => c.key === key);
			return { sortBy: key, sortOrder: column?.defaultDesc ? "desc" : "asc" };
		});
	};

	const openModelOverview = (item: ProviderModelRankItem) => {
		// 携带当前时间段参数（custom 时带起止，否则带 period/offset）。
		const params = new URLSearchParams();
		if (windowState.period === "custom") {
			params.set("period", "custom");
			params.set("startTime", String(window.startTime));
			params.set("endTime", String(window.endTime));
		} else {
			params.set("period", windowState.period);
			params.set("offset", String(windowState.offset));
		}
		navigate(
			`/models/${item.providerId}/${encodeURIComponent(item.modelId)}/overview?${params.toString()}`,
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
					<p className="truncate text-xs text-muted-foreground">
						{windowState.period === "custom"
							? t("overview.customWindow")
							: formatPeriodLabel(windowState.period, windowState.offset, now)}
					</p>
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
				<RaceTable
					items={query.data.items}
					sort={sort}
					onSort={handleSort}
					onRowClick={openModelOverview}
				/>
			)}
		</Card>
	);
}

/** 可排序指标表格；点击行进入模型详情三级页。 */
function RaceTable({
	items,
	sort,
	onSort,
	onRowClick,
}: {
	items: ProviderModelRankItem[];
	sort: RaceSort;
	onSort: (key: RaceSortKey) => void;
	onRowClick: (item: ProviderModelRankItem) => void;
}) {
	const { t } = useTranslation();
	return (
		<div className="overflow-x-auto">
			<table className="w-full min-w-[720px] border-collapse text-sm">
				<thead>
					<tr className="border-b border-foreground/10">
						<th className="w-10 px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							#
						</th>
						<th className="px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							{t("dashboard.providerModel")}
						</th>
						{COLUMNS.map((column) => {
							const active = sort.sortBy === column.key;
							const label = t(column.labelKey);
							return (
								<th key={column.key} className="px-2 py-2 text-right">
									<button
										type="button"
										onClick={() => onSort(column.key)}
										aria-label={label}
										className={`inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs font-medium transition-colors hover:bg-foreground/5 ${
											active ? "text-foreground" : "text-muted-foreground"
										}`}
									>
										{label}
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
					{items.map((item, index) => (
						<tr
							key={`${item.providerName}::${item.modelId}`}
							onClick={() => onRowClick(item)}
							onKeyDown={(e) => {
								if (e.key === "Enter") {
									onRowClick(item);
								}
							}}
							tabIndex={0}
							className="cursor-pointer border-b border-foreground/5 last:border-0 hover:bg-foreground/5"
							title={t("race.openModelDetail")}
						>
							<td className="px-2 py-2 text-left font-mono text-xs text-muted-foreground">
								{index + 1}
							</td>
							<td className="px-2 py-2 text-left font-medium text-foreground">
								{modelLabel(item)}
							</td>
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
