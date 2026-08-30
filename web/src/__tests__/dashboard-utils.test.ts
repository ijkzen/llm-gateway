import { formatContextLength, formatTokenCount, middleEllipsis, topWithOther } from "@/lib/utils";
import { describe, expect, it } from "vitest";

describe("formatTokenCount", () => {
	it("小于 1 万原样千分位展示", () => {
		expect(formatTokenCount(0)).toBe("0");
		expect(formatTokenCount(999)).toBe("999");
		expect(formatTokenCount(9999)).toBe("9,999");
	});

	it("1 万到 1 亿之间用「万」，一位小数并去尾零", () => {
		expect(formatTokenCount(10_000)).toBe("1 万");
		expect(formatTokenCount(12_345)).toBe("1.2 万");
		expect(formatTokenCount(15_000)).toBe("1.5 万");
		expect(formatTokenCount(99_990_000)).toBe("9999 万");
	});

	it("大于等于 1 亿用「亿」，两位小数并去尾零", () => {
		expect(formatTokenCount(100_000_000)).toBe("1 亿");
		expect(formatTokenCount(123_456_789)).toBe("1.23 亿");
		expect(formatTokenCount(1_050_000_000)).toBe("10.5 亿");
	});
});

describe("topWithOther", () => {
	it("数量不超过 limit 时仅排序不合并", () => {
		const items = [
			{ modelId: "a", value: 1 },
			{ modelId: "b", value: 3 },
			{ modelId: "c", value: 2 },
		];
		expect(topWithOther(items, { modelId: "其他", value: 0 })).toEqual([
			{ modelId: "b", value: 3 },
			{ modelId: "c", value: 2 },
			{ modelId: "a", value: 1 },
		]);
	});

	it("超过 limit 时尾部合并为「其他」", () => {
		const items = Array.from({ length: 12 }, (_, i) => ({
			modelId: `m${i}`,
			value: 100 - i,
		}));
		const result = topWithOther(items, { modelId: "其他", value: 0 });
		expect(result).toHaveLength(11);
		expect(result[0]).toEqual({ modelId: "m0", value: 100 });
		// m10(90) 与 m11(89) 合并
		expect(result[10]).toEqual({ modelId: "其他", value: 179 });
	});

	it("恰好等于 limit 时不合并", () => {
		const items = Array.from({ length: 10 }, (_, i) => ({ modelId: `m${i}`, value: i }));
		expect(topWithOther(items, { modelId: "其他", value: 0 })).toHaveLength(10);
	});
});

describe("middleEllipsis", () => {
	it("不超过 maxLength 时原样返回", () => {
		expect(middleEllipsis("deepseek-chat", 16)).toBe("deepseek-chat");
		expect(middleEllipsis("恰好十六个字符呀啊啊啊啊啊", 16)).toBe("恰好十六个字符呀啊啊啊啊啊");
	});

	it("超过时保留首尾、中间以 … 省略", () => {
		expect(middleEllipsis("阿里云・deepseek-chat", 16)).toBe("阿里云・deep…ek-chat");
	});

	it("总长度含省略号，首部比尾部多留一位", () => {
		const result = middleEllipsis("1234567890abcdef", 10);
		expect(result).toBe("12345…cdef");
		expect(result.length).toBe(10);
	});

	it("对代理对（emoji）按码点省略不截断", () => {
		expect(middleEllipsis("😀😀😀😀😀😀", 4)).toBe("😀😀…😀");
	});
});

describe("formatContextLength", () => {
	it("小于 1000 原样返回", () => {
		expect(formatContextLength(0)).toBe("0");
		expect(formatContextLength(999)).toBe("999");
	});

	it("1000 进制 K 缩写（去尾零）", () => {
		expect(formatContextLength(1_000)).toBe("1K");
		expect(formatContextLength(128_000)).toBe("128K");
		expect(formatContextLength(131_072)).toBe("131.1K");
		expect(formatContextLength(200_000)).toBe("200K");
	});

	it("M 缩写", () => {
		expect(formatContextLength(1_000_000)).toBe("1M");
		expect(formatContextLength(1_048_576)).toBe("1M");
		expect(formatContextLength(2_500_000)).toBe("2.5M");
	});
});
