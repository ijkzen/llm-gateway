export const DEFAULT_GROUP = "默认";

export const SETTING_TYPES = ["String", "Int", "Float", "Bool"] as const;

export type SettingType = (typeof SETTING_TYPES)[number];

/** 虚拟模型负载均衡策略（取值与后端 LoadBalancingStrategy 枚举一致）。 */
export const LOAD_BALANCING_STRATEGIES = [
	{ value: 0, label: "订阅制优先" },
	{ value: 1, label: "按量付费优先" },
	{ value: 2, label: "轮转" },
	{ value: 3, label: "随机" },
] as const;

/** 虚拟模型降级策略（取值与后端 FallbackStrategy 枚举一致）。 */
export const FALLBACK_STRATEGIES = [
	{ value: 0, label: "直接失败" },
	{ value: 1, label: "依次重试其他启用成员" },
] as const;

export function loadBalancingLabel(value: number): string {
	return LOAD_BALANCING_STRATEGIES.find((s) => s.value === value)?.label ?? "未知";
}

export function fallbackLabel(value: number): string {
	return FALLBACK_STRATEGIES.find((s) => s.value === value)?.label ?? "未知";
}
