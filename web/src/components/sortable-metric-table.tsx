import { ArrowDown, ArrowUp } from "lucide-react";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";

import type { RaceSort, RaceSortKey } from "@/lib/race-types";
import { type Locale, formatPercent, formatTokenCount, localeOf } from "@/lib/utils";

/** 指标列定义：key / 标题键 / 格式化 / 默认方向（true=降序，耗时类默认升序）。 */
export interface MetricColumn {
	key: RaceSortKey;
	labelKey: string;
	format: (v: number, locale: Locale) => string;
	defaultDesc: boolean;
}

/** 6 指标列定义：总计 Token / 请求数 / TTFT / 平均耗时 / TPS / 缓存命中率。 */
export const RACE_COLUMNS: ReadonlyArray<MetricColumn> = [
	{
		key: "totalTokens",
		labelKey: "race.metricLabel.totalTokens",
		format: (v, locale) => formatTokenCount(v, locale),
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

/** 指标标题键（按 key 取，供自定义列集的表头复用）。 */
export const RACE_COLUMN_LABEL_KEYS: Record<RaceSortKey, string> = Object.fromEntries(
	RACE_COLUMNS.map((c) => [c.key, c.labelKey]),
) as Record<RaceSortKey, string>;

/** 排序状态：默认按总计 Token 降序；点击同列翻转，点新列取该列默认方向。 */
export function useRaceSort(columns: ReadonlyArray<MetricColumn> = RACE_COLUMNS) {
	const [sort, setSort] = useState<RaceSort>({ sortBy: "totalTokens", sortOrder: "desc" });
	const onSort = (key: RaceSortKey) => {
		setSort((prev) => {
			if (prev.sortBy === key) {
				return { ...prev, sortOrder: prev.sortOrder === "asc" ? "desc" : "asc" };
			}
			const column = columns.find((c) => c.key === key);
			return { sortBy: key, sortOrder: column?.defaultDesc ? "desc" : "asc" };
		});
	};
	return { sort, onSort };
}

interface SortableMetricTableProps<T extends Record<RaceSortKey, number>> {
	items: T[];
	sort: RaceSort;
	onSort: (key: RaceSortKey) => void;
	columns?: ReadonlyArray<MetricColumn>;
	/** 名称列表头（i18n key）。 */
	nameHeader: string;
	/** 名称单元格内容（可为富文本，如「供应商・模型 + 停用后缀」）。 */
	renderName: (item: T) => ReactNode;
	/** 行 key。 */
	rowKey: (item: T) => string | number;
	/** 行点击（不传则整表不可点）。 */
	onRowClick?: (item: T) => void;
	/** 行级可点判定（如已删除 Key 的历史行不可点）；默认全部可点。 */
	isRowClickable?: (item: T) => boolean;
	/** 可点行的 title（i18n key）。 */
	rowTitleKey?: string;
	/** 行附加类名（如停用成员灰显）。 */
	rowClassName?: (item: T) => string;
}

/**
 * 可排序指标表格：六指标表头 + 排序翻转指示 + 序号/名称/指标列。
 * 赛马卡与二级/三级页内嵌赛马表共用；行的可点行为与名称渲染由调用方注入。
 */
export function SortableMetricTable<T extends Record<RaceSortKey, number>>({
	items,
	sort,
	onSort,
	columns = RACE_COLUMNS,
	nameHeader,
	renderName,
	rowKey,
	onRowClick,
	isRowClickable,
	rowTitleKey,
	rowClassName,
}: SortableMetricTableProps<T>) {
	const { t, i18n } = useTranslation();
	const locale = localeOf(i18n.language);
	const clickable = (item: T) => (isRowClickable ? isRowClickable(item) : Boolean(onRowClick));
	return (
		<div className="overflow-x-auto">
			<table className="w-full min-w-[720px] border-collapse text-sm">
				<thead>
					<tr className="border-b border-foreground/10">
						<th className="w-10 px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							#
						</th>
						<th className="px-2 py-2 text-left text-xs font-medium text-muted-foreground">
							{t(nameHeader)}
						</th>
						{columns.map((column) => {
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
					{items.map((item, index) => {
						const canClick = clickable(item);
						return (
							<tr
								key={rowKey(item)}
								onClick={canClick ? () => onRowClick?.(item) : undefined}
								onKeyDown={
									canClick
										? (e) => {
												if (e.key === "Enter") {
													onRowClick?.(item);
												}
											}
										: undefined
								}
								tabIndex={canClick ? 0 : undefined}
								title={canClick && rowTitleKey ? t(rowTitleKey) : undefined}
								className={`border-b border-foreground/5 last:border-0 ${
									canClick ? "cursor-pointer hover:bg-foreground/5" : ""
								} ${rowClassName?.(item) ?? ""}`}
							>
								<td className="px-2 py-2 text-left font-mono text-xs text-muted-foreground">
									{index + 1}
								</td>
								<td className="px-2 py-2 text-left font-medium text-foreground">
									{renderName(item)}
								</td>
								{columns.map((column) => (
									<td
										key={column.key}
										className="px-2 py-2 text-right font-mono text-xs tabular-nums text-foreground"
									>
										{column.format(item[column.key], locale)}
									</td>
								))}
							</tr>
						);
					})}
				</tbody>
			</table>
		</div>
	);
}
