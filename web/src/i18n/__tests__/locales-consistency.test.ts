import en from "@/i18n/locales/en";
import zhCN from "@/i18n/locales/zh-CN";
import { describe, expect, it } from "vitest";

/** 取对象全部叶子键路径。 */
function leafKeys(obj: unknown, prefix = ""): string[] {
	if (obj === null || typeof obj !== "object") return prefix ? [prefix] : [];
	return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
		leafKeys(v, prefix ? `${prefix}.${k}` : k),
	);
}

/** 取嵌套键对应的值。 */
function valueAt(obj: unknown, key: string): unknown {
	return key
		.split(".")
		.reduce<unknown>((acc, part) => (acc as Record<string, unknown>)?.[part], obj);
}

/** 提取文案里的 {{占位符}} 名集合。 */
function placeholders(value: unknown): Set<string> {
	if (typeof value !== "string") return new Set();
	return new Set(Array.from(value.matchAll(/\{\{(\w+)\}\}/g)).map((m) => m[1] as string));
}

// 源码文本（?raw）：用于「引用的 t("域.key") 必须存在」校验。
// 用 Vite 的 import.meta.glob 读取（前端 tsconfig 无 node 类型，故不走 fs）。
const SOURCES = import.meta.glob("../../**/*.{ts,tsx}", {
	query: "?raw",
	import: "default",
	eager: true,
}) as Record<string, string>;

const ZH_KEYS = new Set(leafKeys(zhCN));
const EN_KEYS = new Set(leafKeys(en));

describe("i18n 键集合一致性（20-07）", () => {
	it("中英键集合完全一致", () => {
		const onlyZh = [...ZH_KEYS].filter((k) => !EN_KEYS.has(k));
		const onlyEn = [...EN_KEYS].filter((k) => !ZH_KEYS.has(k));
		expect(onlyZh).toEqual([]);
		expect(onlyEn).toEqual([]);
	});

	it("同键占位符集合一致", () => {
		const mismatched: string[] = [];
		for (const key of ZH_KEYS) {
			const zh = placeholders(valueAt(zhCN, key));
			const enPlaceholders = placeholders(valueAt(en, key));
			const same =
				zh.size === enPlaceholders.size && [...zh].every((name) => enPlaceholders.has(name));
			if (!same) mismatched.push(key);
		}
		expect(mismatched).toEqual([]);
	});

	it('源码引用的 t("...") 键都存在于 locales', () => {
		const missing = new Set<string>();
		for (const [file, text] of Object.entries(SOURCES)) {
			if (file.includes("i18n/locales") || file.includes("__tests__")) continue;
			for (const match of text.matchAll(/\bt\(\s*"([a-zA-Z0-9_.]+)"/g)) {
				const key = match[1] as string;
				// 只校验形如「域.key」的多段键（单段多为普通字符串误匹配）。
				if (!key.includes(".")) continue;
				if (!ZH_KEYS.has(key)) missing.add(`${key} (${file})`);
			}
		}
		expect([...missing]).toEqual([]);
	});
});
