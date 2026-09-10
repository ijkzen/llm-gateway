import { formatBucketLabel } from "@/components/dashboard-charts";
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
