import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import type { ApiKeyRaceFilter } from "@/hooks/use-api-key-race";

/**
 * API Key 赛马区块：单卡片展示全部 API Key（或按过滤维度）的 6 个指标
 * （总计 Token / 请求数 / TTFT / 平均耗时 / TPS / 缓存命中率），时间窗口
 * 天/周/月/年/自定义，卡片进入视口才发起请求（懒加载）。
 */
export function ApiKeyRaceSection({ filter }: { filter?: ApiKeyRaceFilter }) {
	return <ApiKeyRaceCard filter={filter} />;
}
