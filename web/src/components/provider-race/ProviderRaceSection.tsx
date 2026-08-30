import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";

/**
 * 供应商赛马区块：单卡片展示全部供应商的 6 个指标（总计 Token / 请求数 /
 * TTFT / 平均耗时 / TPS / 缓存命中率），时间窗口天/周/月/年/自定义，
 * 卡片进入视口才发起请求（懒加载）。
 */
export function ProviderRaceSection() {
	return <ProviderRaceCard />;
}
