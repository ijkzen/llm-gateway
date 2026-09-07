import { RaceCardShell, useRaceCardWindow } from "@/components/race-card-shell";
import { type RaceWindowState, windowQueryString } from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import { type ProviderModelRankItem, useProviderModelRace } from "@/hooks/use-provider-model-race";
import { Boxes } from "lucide-react";
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
	const view = useRaceCardWindow(initialWindow);

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const { sort, onSort } = useRaceSort();

	const query = useProviderModelRace(view.window, sort, view.inView, undefined, apiKey);

	const openModelOverview = (item: ProviderModelRankItem) => {
		if (item.modelPk === null || item.modelPk === undefined) {
			return;
		}
		navigate(
			`/models/${item.modelPk}/overview?${windowQueryString(view.windowState, view.window)}`,
		);
	};

	return (
		<RaceCardShell
			view={view}
			icon={Boxes}
			titleKey="dashboard.providerModelRace"
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
				nameHeader="dashboard.providerModel"
				renderName={modelLabel}
				rowKey={(item) => `${item.providerName}::${item.modelId}`}
				onRowClick={openModelOverview}
				isRowClickable={(item) => item.modelPk !== null && item.modelPk !== undefined}
				rowTitleKey="race.openModelDetail"
			/>
		</RaceCardShell>
	);
}
