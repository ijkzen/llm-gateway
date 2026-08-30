import { ProviderModelRaceCard } from "@/components/provider-model-race/ProviderModelRaceCard";

/**
 * 供应商模型平铺赛马区块：单卡片展示全部「供应商×模型」的 6 个指标
 * （总计 Token / 请求数 / TTFT / 平均耗时 / TPS / 缓存命中率），时间窗口
 * 天/周/月/年/自定义，卡片进入视口才发起请求（懒加载）。
 */
export function ProviderModelRaceSection() {
	return <ProviderModelRaceCard />;
}
