import i18n from "@/i18n";

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

export function loadBalancingLabel(value: number): string {
	return i18n.t(
		LOAD_BALANCING_STRATEGIES.find((s) => s.value === value)?.labelKey ?? "common.unknown",
	);
}

export function fallbackLabel(value: number): string {
	return i18n.t(FALLBACK_STRATEGIES.find((s) => s.value === value)?.labelKey ?? "common.unknown");
}
