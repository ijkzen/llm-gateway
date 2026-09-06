import { RaceCardShell, useRaceCardWindow } from "@/components/race-card-shell";
import { windowQueryString } from "@/components/race-window-control";
import { SortableMetricTable, useRaceSort } from "@/components/sortable-metric-table";
import {
	type ApiKeyRaceFilter,
	type ApiKeyRankItem,
	useApiKeyRace,
} from "@/hooks/use-api-key-race";
import { KeyRound } from "lucide-react";
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
	const view = useRaceCardWindow();

	// 排序：默认按总计 Token 降序；点击表头切换升/降。
	const { sort, onSort } = useRaceSort();

	const query = useApiKeyRace(view.window, sort, view.inView, filter);

	const openApiKeyOverview = (item: ApiKeyRankItem) => {
		if (item.apiKeyId === null || item.apiKeyId === undefined) {
			return;
		}
		navigate(
			`/api-keys/${item.apiKeyId}/overview?${windowQueryString(view.windowState, view.window)}`,
		);
	};

	return (
		<RaceCardShell
			view={view}
			icon={KeyRound}
			titleKey="dashboard.apiKeyRace"
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
				nameHeader="dashboard.apiKeyColumn"
				renderName={(item) => item.apiKeyName || t("race.unknownKey")}
				rowKey={(item) => item.apiKeyName}
				onRowClick={openApiKeyOverview}
				isRowClickable={(item) => item.apiKeyId !== null && item.apiKeyId !== undefined}
				rowTitleKey="race.openApiKeyOverview"
			/>
		</RaceCardShell>
	);
}
