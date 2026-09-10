import {
	cn,
	formatDateTime,
	formatPercent,
	getPageNumbers,
	localeOf,
	middleEllipsis,
} from "@/lib/utils";
import { describe, expect, it } from "vitest";

describe("getPageNumbers 分页号码序列（19-17）", () => {
	it("总页数不超过 5：全部显示", () => {
		expect(getPageNumbers(1, 1)).toEqual([1]);
		expect(getPageNumbers(3, 5)).toEqual([1, 2, 3, 4, 5]);
	});

	it("靠近开头：前 4 页 + 省略号 + 末页", () => {
		expect(getPageNumbers(1, 10)).toEqual([1, 2, 3, 4, "...", 10]);
		expect(getPageNumbers(3, 10)).toEqual([1, 2, 3, 4, "...", 10]);
	});

	it("靠近结尾：首页 + 省略号 + 末 4 页", () => {
		expect(getPageNumbers(8, 10)).toEqual([1, "...", 7, 8, 9, 10]);
		expect(getPageNumbers(10, 10)).toEqual([1, "...", 7, 8, 9, 10]);
	});

	it("中间：首页 + 省略号 + 当前前后各一页 + 省略号 + 末页", () => {
		expect(getPageNumbers(5, 10)).toEqual([1, "...", 4, 5, 6, "...", 10]);
		expect(getPageNumbers(6, 12)).toEqual([1, "...", 5, 6, 7, "...", 12]);
	});

	it("边界处不产生越界或重复页码", () => {
		// currentPage = totalPages - 2 与 = 3 是两分支的交界，行为钉死。
		expect(getPageNumbers(3, 6)).toEqual([1, 2, 3, 4, "...", 6]);
		expect(getPageNumbers(4, 6)).toEqual([1, "...", 3, 4, 5, 6]);
	});
});

describe("formatPercent", () => {
	it("按百分比保留必要精度", () => {
		expect(formatPercent(0.32)).toBe("32%");
		expect(formatPercent(0)).toBe("0%");
		expect(formatPercent(1)).toBe("100%");
		expect(formatPercent(0.123456)).toBe("12.3456%");
	});
});

describe("formatDateTime（19-20：locale 参数）", () => {
	it("空串/非法/零值返回占位符", () => {
		expect(formatDateTime("")).toBe("—");
		expect(formatDateTime("not-a-date")).toBe("—");
		expect(formatDateTime("1970-01-01T00:00:00Z")).toBe("—");
	});

	it("缺省为中文格式，locale=en 走英文格式", () => {
		const iso = "2026-08-31T12:00:00Z";
		const zh = formatDateTime(iso);
		const en = formatDateTime(iso, "en");
		expect(zh).not.toBe("—");
		expect(en).not.toBe("—");
		// 两种 locale 的展示格式不同（中文「2026/8/31 20:00:00」类 vs 英文带 AM/PM）。
		expect(en).not.toBe(zh);
	});
});

describe("localeOf", () => {
	it("zh 前缀映射 zh，其余映射 en", () => {
		expect(localeOf("zh-CN")).toBe("zh");
		expect(localeOf("zh")).toBe("zh");
		expect(localeOf("en")).toBe("en");
		expect(localeOf("en-US")).toBe("en");
	});
});

describe("cn 类名合并", () => {
	it("拼接且后者覆盖同族 Tailwind 类", () => {
		expect(cn("p-2", "p-4")).toBe("p-4");
		expect(cn("text-sm", false && "hidden", undefined, "font-bold")).toBe("text-sm font-bold");
	});
});

describe("middleEllipsis 长度口径（19-16：按码点）", () => {
	it("未超长原样返回", () => {
		expect(middleEllipsis("abc", 5)).toBe("abc");
	});

	it("超长时中间省略且长度不超上限", () => {
		const result = middleEllipsis("阿里云・deepseek-chat", 10);
		expect(result).toContain("…");
		expect(Array.from(result).length).toBeLessThanOrEqual(10);
	});

	it("含代理对（emoji）时按码点计数不超过上限", () => {
		const result = middleEllipsis("🎉🎉🎉🎉🎉🎉🎉🎉", 4);
		expect(Array.from(result).length).toBeLessThanOrEqual(4);
	});
});
