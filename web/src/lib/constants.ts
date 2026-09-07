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

/** 虚拟模型接口类型（取值与后端对齐：0-2 与供应商协议编号一致，4=Full Compatible；3=Gemini 为后端保留值，暂不开放）。 */
export const INTERFACE_TYPES = [
	{ value: 0, labelKey: "virtualModels.interfaceTypes.openaiCompat" },
	{ value: 1, labelKey: "virtualModels.interfaceTypes.responses" },
	{ value: 2, labelKey: "virtualModels.interfaceTypes.messages" },
	{ value: 4, labelKey: "virtualModels.interfaceTypes.fullCompatible" },
] as const;

/** Full Compatible：接受任意协议成员、由 /v1/chat/completions 转换服务。 */
export const INTERFACE_FULL_COMPATIBLE = 4;

/** 策略标签经调用方注入翻译函数（保持本模块无全局 i18n 读取）。 */
export function loadBalancingLabel(value: number, t: (key: string) => string): string {
	return t(LOAD_BALANCING_STRATEGIES.find((s) => s.value === value)?.labelKey ?? "common.unknown");
}

export function fallbackLabel(value: number, t: (key: string) => string): string {
	return t(FALLBACK_STRATEGIES.find((s) => s.value === value)?.labelKey ?? "common.unknown");
}

export function interfaceTypeLabel(value: number, t: (key: string) => string): string {
	return t(INTERFACE_TYPES.find((s) => s.value === value)?.labelKey ?? "common.unknown");
}
