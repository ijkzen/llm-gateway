import { MetricRaceCard, raceHref } from "@/components/metric-race-card";
import type { RaceWindowState } from "@/components/race-window-control";
import {
	type ApiKeyRaceFilter,
	type ApiKeyRankItem,
	useApiKeyRace,
} from "@/hooks/use-api-key-race";
import { KeyRound } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

/** 行可点判定：已删除 Key 的历史聚合行无主键，不可点击。 */
function isRowClickable(item: ApiKeyRankItem): boolean {
	return item.apiKeyId !== null && item.apiKeyId !== undefined;
}

/**
 * API Key 赛马卡片：按调用方 API Key 聚合展示 6 个指标（总计 Token / 请求数 /
 * TTFT / 平均耗时 / TPS / 缓存命中率），可点表头按任意指标升/降序；时间窗口
 * 支持天/周/月/年（左右箭头切换周期）+ 自定义（秒级）。卡片进入视口才发请求。
 * 可选按供应商/虚拟模型/模型过滤（二级/三级页）。现存 Key 的行可点击进入其
 * 数据面板（携带当前时间段参数）；已删除 Key 的历史聚合行无主键，不可点击。
 * 可选初始时间窗（16-09：与另三张卡签名对齐，供数据面板接收 URL 参数）。
 */
export function ApiKeyRaceCard({
	filter,
	initialWindow,
}: {
	/** 过滤维度：首页不传（全量），二级/三级页按需传。 */
	filter?: ApiKeyRaceFilter;
	/** 初始时间窗（未传默认当天）。 */
	initialWindow?: RaceWindowState;
}) {
	const navigate = useNavigate();
	const { t } = useTranslation();

	return (
		<MetricRaceCard<ApiKeyRankItem>
			icon={KeyRound}
			titleKey="dashboard.apiKeyRace"
			nameHeader="dashboard.apiKeyColumn"
			initialWindow={initialWindow}
			useQuery={(view, sort, inView) => useApiKeyRace(view.window, sort, inView, filter)}
			renderName={(item) => item.apiKeyName || t("race.unknownKey")}
			rowKey={(item) => item.apiKeyName}
			onRowClick={(item, view) => {
				if (item.apiKeyId === null || item.apiKeyId === undefined) {
					return;
				}
				navigate(raceHref(`/api-keys/${item.apiKeyId}/overview`, view.windowState, view.window));
			}}
			isRowClickable={isRowClickable}
			rowTitleKey="race.openApiKeyOverview"
		/>
	);
}
