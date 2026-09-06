import { makeProviderModelBaseSchema } from "@/components/provider-models/provider-model-form";
import { proxySuperRefine } from "@/components/providers/ProxyConfigFields";
import { describe, expect, it } from "vitest";
import { z } from "zod";

const t = (key: string) => key;

describe("makeProviderModelBaseSchema", () => {
	const schema = makeProviderModelBaseSchema(t);

	it("合法值通过", () => {
		const ok = schema.safeParse({
			providerModelId: "glm-4.5",
			contextLength: "128000",
			maxOutputTokens: 8192,
			reasoning: true,
			toolUse: false,
			imageUnderstand: false,
			videoUnderstand: false,
		});
		expect(ok.success).toBe(true);
		if (ok.success) {
			expect(ok.data.contextLength).toBe(128000);
		}
	});

	it("模型 ID 必填", () => {
		const result = schema.safeParse({
			providerModelId: "",
			contextLength: 1,
			maxOutputTokens: 1,
			reasoning: false,
			toolUse: false,
			imageUnderstand: false,
			videoUnderstand: false,
		});
		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error.issues[0]?.message).toBe("providerModels.modelIdRequired");
		}
	});

	it("上下文长度须为正整数（字符串数字亦可）", () => {
		const base = {
			providerModelId: "m",
			maxOutputTokens: 1,
			reasoning: false,
			toolUse: false,
			imageUnderstand: false,
			videoUnderstand: false,
		};
		for (const bad of [0, -5, 1.5, "abc"]) {
			expect(schema.safeParse({ ...base, contextLength: bad }).success).toBe(false);
		}
		const bad = schema.safeParse({ ...base, contextLength: 0 });
		expect(bad.success).toBe(false);
		if (!bad.success) {
			expect(bad.error.issues[0]?.message).toBe("providerModels.mustBePositive");
		}
	});
});

describe("proxySuperRefine", () => {
	const schema = z
		.object({ proxyEnabled: z.boolean(), proxyAddr: z.string() })
		.superRefine(proxySuperRefine(t));

	it("关闭时不校验地址", () => {
		expect(schema.safeParse({ proxyEnabled: false, proxyAddr: "" }).success).toBe(true);
	});

	it("开启时地址必填", () => {
		const result = schema.safeParse({ proxyEnabled: true, proxyAddr: "  " });
		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error.issues[0]?.message).toBe("providers.proxyAddrRequired");
			expect(result.error.issues[0]?.path).toEqual(["proxyAddr"]);
		}
	});

	it("开启时地址须 http:// 开头", () => {
		const result = schema.safeParse({ proxyEnabled: true, proxyAddr: "https://x" });
		expect(result.success).toBe(false);
		if (!result.success) {
			expect(result.error.issues[0]?.message).toBe("providers.proxyAddrInvalid");
		}
		expect(
			schema.safeParse({ proxyEnabled: true, proxyAddr: "http://10.0.0.1:8080" }).success,
		).toBe(true);
	});
});
