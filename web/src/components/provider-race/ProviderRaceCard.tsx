import { RaceCardShell, useRaceCardWindow } from "@/components/race-card-shell";
import { type RaceWindowState, windowQueryString } from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import { type ProviderRankItem, useProviderRace } from "@/hooks/use-provider-race";
import { TrendingUp } from "lucide-react";
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
	const view = useRaceCardWindow(initialWindow);

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const { sort, onSort } = useRaceSort();

	const query = useProviderRace(view.window, sort, view.inView, apiKey);

	const openProviderOverview = (item: ProviderRankItem) => {
		navigate(
			`/providers/${item.providerId}/overview?${windowQueryString(view.windowState, view.window)}`,
		);
	};

	return (
		<RaceCardShell
			view={view}
			icon={TrendingUp}
			titleKey="dashboard.providerRace"
			status={{
				isLoading: query.isLoading,
				isError: query.isError,
				isEmpty: !query.data || query.data.items.length === 0,
				onRetry: () => query.refetch(),
			}}
		>
			<SortableMetricTable
				items={query.data?.items ?? []}
				sort={sort}
				onSort={onSort}
				nameHeader="dashboard.providerColumn"
				renderName={(item) => item.providerName || t("race.unknownProvider")}
				rowKey={(item) => item.providerId}
				onRowClick={openProviderOverview}
				rowTitleKey="race.openProviderOverview"
			/>
		</RaceCardShell>
	);
}
