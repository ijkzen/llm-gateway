import {
	formatBucketLabel,
	inferGranularity,
	labelInterval,
	toRankedModels,
} from "@/components/dashboard-charts";
import { describe, expect, it } from "vitest";

describe("formatBucketLabel X 轴标签", () => {
	it("en 分支输出英文月份/日期", () => {
		const ms = new Date(2026, 7, 31).getTime();
		expect(formatBucketLabel(ms, "day", "en")).toBe("Aug 31");
		expect(formatBucketLabel(ms, "month", "en")).toBe("Aug 2026");
	});

	// 2026-08-31 12:00 本地时区的毫秒时间戳。
	const ms = new Date(2026, 7, 31, 12, 0, 0).getTime();

	it("小时 → HH:00", () => {
		expect(formatBucketLabel(ms, "hour", "zh")).toBe("12:00");
		const midnight = new Date(2026, 7, 31, 0, 0).getTime();
		expect(formatBucketLabel(midnight, "hour", "zh")).toBe("00:00");
	});

	it("天 → M月d日", () => {
		expect(formatBucketLabel(ms, "day", "zh")).toBe("8月31日");
	});

	it("月 → yyyy年M月", () => {
		expect(formatBucketLabel(ms, "month", "zh")).toBe("2026年8月");
	});

	it("年 → yyyy年", () => {
		expect(formatBucketLabel(ms, "year", "zh")).toBe("2026年");
		const jan = new Date(2025, 0, 1).getTime();
		expect(formatBucketLabel(jan, "year", "zh")).toBe("2025年");
	});

	it("formatBucketLabel 按 IANA 时区取墙钟（16-06）", () => {
		// 2026-08-31T00:00:00Z：上海为 08:00，纽约为 20:00（前一日）。
		const utcMidnight = Date.UTC(2026, 7, 31, 0, 0, 0);
		expect(formatBucketLabel(utcMidnight, "hour", "zh", "Asia/Shanghai")).toBe("08:00");
		expect(formatBucketLabel(utcMidnight, "hour", "zh", "America/New_York")).toBe("20:00");
		expect(formatBucketLabel(utcMidnight, "day", "zh", "Asia/Shanghai")).toBe("8月31日");
		expect(formatBucketLabel(utcMidnight, "day", "zh", "America/New_York")).toBe("8月30日");
	});
});

describe("inferGranularity 桶粒度推断（16-17 回归）", () => {
	it("按相邻桶间距推断小时/天/月/年", () => {
		const base = Date.UTC(2026, 0, 1);
		const hour = 3_600_000;
		expect(inferGranularity([base, base + hour])).toBe("hour");
		expect(inferGranularity([base, base + 24 * hour])).toBe("day");
		expect(inferGranularity([base, base + 30 * 24 * hour])).toBe("month");
		// 16-14：>90 天按年桶（与 chartGranularity 的 >366d→year 口径对齐）。
		expect(inferGranularity([base, base + 365 * 24 * hour])).toBe("year");
	});

	it("点数不足或为空时回退月桶", () => {
		expect(inferGranularity([])).toBe("month");
		expect(inferGranularity([Date.UTC(2026, 0, 1)])).toBe("month");
	});
});

describe("labelInterval X 轴标签密度（16-12 共用 helper）", () => {
	it("约每 6 个点显示一个标签，最少 0", () => {
		expect(labelInterval(0)).toBe(0);
		expect(labelInterval(6)).toBe(0);
		expect(labelInterval(12)).toBe(1);
		expect(labelInterval(24)).toBe(3);
		expect(labelInterval(31)).toBe(4);
	});
});

describe("toRankedModels Top10 + 其他聚合（16-17 回归）", () => {
	const model = (modelId: string, value: number) => ({
		providerName: "供应商A",
		modelId,
		value,
	});

	it("不超过 10 项时全量返回且按值降序", () => {
		const items = [model("a", 3), model("b", 9), model("c", 1)];
		const ranked = toRankedModels(items, "其他");
		expect(ranked.map((r) => r.modelId)).toEqual(["b", "a", "c"]);
		expect(ranked.some((r) => r.label === "其他")).toBe(false);
	});

	it("超过 10 项时第 11 项起合并为「其他」并累计求和", () => {
		const items = Array.from({ length: 12 }, (_, i) => model(`m${String(i)}`, 12 - i));
		const ranked = toRankedModels(items, "其他");
		expect(ranked).toHaveLength(11);
		const other = ranked[ranked.length - 1];
		expect(other?.label).toBe("其他");
		// 第 11、12 名各 2、1 分。
		expect(other?.value).toBe(3);
	});

	it("展示标签为「供应商・模型」，供应商缺失时退化为模型名", () => {
		const ranked = toRankedModels(
			[model("gpt-4o", 5), { providerName: "", modelId: "orphan", value: 1 }],
			"其他",
		);
		expect(ranked[0]?.label).toBe("供应商A・gpt-4o");
		expect(ranked[1]?.label).toBe("orphan");
	});
});
