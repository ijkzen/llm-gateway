export const DEFAULT_GROUP = "默认";

export const SETTING_TYPES = ["String", "Int", "Float", "Bool", "Json"] as const;

export type SettingType = (typeof SETTING_TYPES)[number];

/** 虚拟模型负载均衡策略（取值与后端 LoadBalancingStrategy 枚举一致）。 */
export const LOAD_BALANCING_STRATEGIES = [
	{ value: 0, labelKey: "virtualModels.strategies.subscriptionFirst" },
	{ value: 1, labelKey: "virtualModels.strategies.paygFirst" },
	{ value: 2, labelKey: "virtualModels.strategies.roundRobin" },
	{ value: 3, labelKey: "virtualModels.strategies.random" },
] as const;

/** 虚拟模型降级策略（取值与后端 FallbackStrategy 枚举一致）。 */
export const FALLBACK_STRATEGIES = [
	{ value: 0, labelKey: "virtualModels.strategies.failFast" },
	{ value: 1, labelKey: "virtualModels.strategies.retryOthers" },
] as const;

/** 策略标签经调用方注入翻译函数（保持本模块无全局 i18n 读取）。 */
export function loadBalancingLabel(value: number, t: (key: string) => string): string {
	return t(LOAD_BALANCING_STRATEGIES.find((s) => s.value === value)?.labelKey ?? "common.unknown");
}

export function fallbackLabel(value: number, t: (key: string) => string): string {
	return t(FALLBACK_STRATEGIES.find((s) => s.value === value)?.labelKey ?? "common.unknown");
}
