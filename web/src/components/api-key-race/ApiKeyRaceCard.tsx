import { MidEllipsis } from "@/components/mid-ellipsis";
import {
	RaceWindowControl,
	defaultRaceWindowState,
	raceWindowBounds,
	windowQueryString,
} from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
	type ApiKeyRaceFilter,
	type ApiKeyRankItem,
	useApiKeyRace,
} from "@/hooks/use-api-key-race";
import { useInView } from "@/hooks/use-in-view";
import { formatPeriodLabel } from "@/lib/race-period";
import { KeyRound } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

/**
 * API Key 赛马卡片：按调用方 API Key 聚合展示 6 个指标（总计 Token / 请求数 /
 * TTFT / 平均耗时 / TPS / 缓存命中率），可点表头按任意指标升/降序；时间窗口
 * 支持天/周/月/年（左右箭头切换周期）+ 自定义（秒级）。卡片进入视口才发请求。
 * 可选按供应商/虚拟模型/模型过滤（二级/三级页）。现存 Key 的行可点击进入其
 * 数据面板（携带当前时间段参数）；已删除 Key 的历史聚合行无主键，不可点击。
 */
export function ApiKeyRaceCard({
	filter,
}: {
	/** 过滤维度：首页不传（全量），二级/三级页按需传。 */
	filter?: ApiKeyRaceFilter;
}) {
	const navigate = useNavigate();
	const { t } = useTranslation();
	// 挂载时刻固化 now：保证「当前周期」的窗口终点稳定，不因渲染抖动重复请求。
	const [now] = useState(() => Date.now());
	const [windowState, setWindowState] = useState(defaultRaceWindowState);

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const { sort, onSort } = useRaceSort();

	const window = raceWindowBounds(windowState, now);

	const { ref, inView } = useInView();
	const query = useApiKeyRace(window, sort, inView, filter);

	const openApiKeyOverview = (item: ApiKeyRankItem) => {
		if (item.apiKeyId === null || item.apiKeyId === undefined) {
			return;
		}
		navigate(`/api-keys/${item.apiKeyId}/overview?${windowQueryString(windowState, window)}`);
	};

	return (
		<Card ref={ref} className="p-5">
			<div className="mb-4 flex flex-wrap items-center gap-3">
				<span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
					<KeyRound className="h-4 w-4" />
				</span>
				<div className="min-w-0">
					<h3 className="text-sm font-semibold text-foreground">{t("dashboard.apiKeyRace")}</h3>
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
					nameHeader="dashboard.apiKeyColumn"
					renderName={(item) => item.apiKeyName || t("race.unknownKey")}
					rowKey={(item) => item.apiKeyName}
					onRowClick={openApiKeyOverview}
					isRowClickable={(item) => item.apiKeyId !== null && item.apiKeyId !== undefined}
					rowTitleKey="race.openApiKeyOverview"
				/>
			)}
		</Card>
	);
}
