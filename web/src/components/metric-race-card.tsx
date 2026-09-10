import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import {
	RaceCardShell,
	type RaceCardWindow,
	useRaceCardWindow,
} from "@/components/race-card-shell";
import { type RaceWindowState, windowQueryString } from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import type { RaceSort, RaceSortKey } from "@/lib/race-types";

/**
 * 赛马卡通用骨架（16-10）：时间窗口 → 排序 → 数据 hook → shell → 表格。
 * 四张赛马卡（供应商 / 虚拟模型 / 供应商模型 / API Key）此前各 ~65 行逐字同构，
 * 差异只有：数据 hook、图标、标题/名称列 i18n key、行渲染与点击行为。
 * 这些差异全部经 props 注入，hooks 接口与各卡对外 props 均不变。
 */
export interface MetricRaceCardProps<T extends Record<RaceSortKey, number>> {
	/** 卡片图标。 */
	icon: LucideIcon;
	/** 卡片标题 i18n key。 */
	titleKey: string;
	/** 名称列表头 i18n key。 */
	nameHeader: string;
	/** 初始时间窗（未传默认当天）。 */
	initialWindow?: RaceWindowState;
	/** 数据查询：接收查询窗口视图、排序与「是否进入视口」。 */
	useQuery: (
		view: RaceCardWindow,
		sort: RaceSort,
		inView: boolean,
	) => { data?: { items: T[] }; isLoading: boolean; isError: boolean; refetch: () => void };
	/** 行 key。 */
	rowKey: (item: T) => string | number;
	/** 名称单元格内容。 */
	renderName: (item: T) => ReactNode;
	/** 行点击（不传则整表不可点）；第二参为窗口视图，供跳转拼接时间段参数。 */
	onRowClick?: (item: T, view: RaceCardWindow) => void;
	/** 行级可点判定。 */
	isRowClickable?: (item: T) => boolean;
	/** 可点行的 title（i18n key）。 */
	rowTitleKey?: string;
	/** 行附加类名。 */
	rowClassName?: (item: T) => string;
}

export function MetricRaceCard<T extends Record<RaceSortKey, number>>({
	icon,
	titleKey,
	nameHeader,
	initialWindow,
	useQuery,
	rowKey,
	renderName,
	onRowClick,
	isRowClickable,
	rowTitleKey,
	rowClassName,
}: MetricRaceCardProps<T>) {
	const view = useRaceCardWindow(initialWindow);
	const { sort, onSort } = useRaceSort();
	const query = useQuery(view, sort, view.inView);

	return (
		<RaceCardShell
			view={view}
			icon={icon}
			titleKey={titleKey}
			status={{
				isLoading: query.isLoading,
				isError: query.isError,
				isEmpty: !query.data || query.data.items.length === 0,
				onRetry: () => query.refetch(),
			}}
		>
			<SortableMetricTable<T>
				items={query.data?.items ?? []}
				sort={sort}
				onSort={onSort}
				nameHeader={nameHeader}
				renderName={renderName}
				rowKey={rowKey}
				onRowClick={onRowClick && ((item) => onRowClick(item, view))}
				isRowClickable={isRowClickable}
				rowTitleKey={rowTitleKey}
				rowClassName={rowClassName}
			/>
		</RaceCardShell>
	);
}

/** 行点击跳转的 URL 构造（四卡共用的窗口参数拼接）。 */
export function raceHref(
	path: string,
	window: RaceWindowState,
	bounds: { startTime: number; endTime: number },
): string {
	return `${path}?${windowQueryString(window, bounds)}`;
}
