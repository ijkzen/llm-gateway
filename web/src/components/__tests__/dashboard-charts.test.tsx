import { formatBucketLabel } from "@/components/dashboard-charts";
import { describe, expect, it } from "vitest";

describe("formatBucketLabel X 轴标签", () => {
	// 2026-08-31 12:00 本地时区的毫秒时间戳。
	const ms = new Date(2026, 7, 31, 12, 0, 0).getTime();

	it("小时 → HH:00", () => {
		expect(formatBucketLabel(ms, "hour")).toBe("12:00");
		const midnight = new Date(2026, 7, 31, 0, 0).getTime();
		expect(formatBucketLabel(midnight, "hour")).toBe("00:00");
	});

	it("天 → M月d日", () => {
		expect(formatBucketLabel(ms, "day")).toBe("8月31日");
	});

	it("月 → yyyy年M月", () => {
		expect(formatBucketLabel(ms, "month")).toBe("2026年8月");
	});

	it("年 → yyyy年", () => {
		expect(formatBucketLabel(ms, "year")).toBe("2026年");
		const jan = new Date(2025, 0, 1).getTime();
		expect(formatBucketLabel(jan, "year")).toBe("2025年");
	});
});
