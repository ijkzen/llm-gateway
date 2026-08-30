import { formatPeriodLabel, periodBounds, toLocalInputValue } from "@/lib/race-period";
import { describe, expect, it } from "vitest";

/**
 * 固定锚点：2026-08-30（周日）本地时区，用于稳定断言。
 * 注意：测试在运行机器本地时区执行，以下期望按本地时区推导。
 */
const NOW_MS = new Date(2026, 7, 30, 15, 30, 0).getTime(); // 2026-08-30 15:30 本地

describe("periodBounds 自然周期", () => {
	it("当前天：[当日0点, now]", () => {
		const { startTime: start, endTime: end } = periodBounds("day", 0, NOW_MS);
		expect(new Date(start).getHours()).toBe(0);
		expect(new Date(start).getMinutes()).toBe(0);
		expect(end).toBe(NOW_MS);
	});

	it("昨天：完整一天", () => {
		const { startTime: start, endTime: end } = periodBounds("day", -1, NOW_MS);
		expect(end - start).toBe(24 * 60 * 60 * 1000);
		expect(new Date(start).getDate()).toBe(29);
		expect(new Date(end).getDate()).toBe(30);
	});

	it("明天：完整一天", () => {
		const { startTime: start, endTime: end } = periodBounds("day", 1, NOW_MS);
		expect(end - start).toBe(24 * 60 * 60 * 1000);
		expect(new Date(start).getDate()).toBe(31);
	});

	it("当前周：周一为起点，终点截到 now", () => {
		const { startTime: start, endTime: end } = periodBounds("week", 0, NOW_MS);
		// 2026-08-30 是周日，所在周的周一是 08-24。
		expect(new Date(start).getDay()).toBe(1);
		expect(new Date(start).getDate()).toBe(24);
		expect(end).toBe(NOW_MS);
	});

	it("上一周：完整 7 天，起点周一", () => {
		const { startTime: start, endTime: end } = periodBounds("week", -1, NOW_MS);
		expect(end - start).toBe(7 * 24 * 60 * 60 * 1000);
		expect(new Date(start).getDay()).toBe(1);
		expect(new Date(start).getDate()).toBe(17);
	});

	it("周日当天偏移 -1 与 0 之间跨周正确", () => {
		// 周日 08-30 的「上一周」起点应是 08-17（周一）。
		const { startTime: start } = periodBounds("week", -1, NOW_MS);
		expect(new Date(start).getDate()).toBe(17);
	});

	it("当前月：月初为起点，终点截到 now", () => {
		const { startTime: start, endTime: end } = periodBounds("month", 0, NOW_MS);
		expect(new Date(start).getDate()).toBe(1);
		expect(end).toBe(NOW_MS);
	});

	it("上个月：完整自然月", () => {
		const { startTime: start, endTime: end } = periodBounds("month", -1, NOW_MS);
		expect(new Date(start).getMonth()).toBe(6); // 7 月
		expect(new Date(start).getDate()).toBe(1);
		expect(new Date(end).getMonth()).toBe(7); // 8 月
		expect(new Date(end).getDate()).toBe(1);
	});

	it("跨年偏移：去年 12 月的下月是今年 1 月", () => {
		const january = new Date(2026, 0, 15, 10, 0).getTime();
		const { startTime: start, endTime: end } = periodBounds("month", -1, january);
		expect(new Date(start).getFullYear()).toBe(2025);
		expect(new Date(start).getMonth()).toBe(11);
		expect(new Date(end).getFullYear()).toBe(2026);
		expect(new Date(end).getMonth()).toBe(0);
	});

	it("当前年：年初为起点，终点截到 now", () => {
		const { startTime: start, endTime: end } = periodBounds("year", 0, NOW_MS);
		expect(new Date(start).getFullYear()).toBe(2026);
		expect(new Date(start).getMonth()).toBe(0);
		expect(new Date(start).getDate()).toBe(1);
		expect(end).toBe(NOW_MS);
	});

	it("去年：完整自然年", () => {
		const { startTime: start, endTime: end } = periodBounds("year", -1, NOW_MS);
		expect(new Date(start).getFullYear()).toBe(2025);
		expect(new Date(end).getFullYear()).toBe(2026);
		expect(end - start).toBe(365 * 24 * 60 * 60 * 1000);
	});
});

describe("formatPeriodLabel", () => {
	it("当前天带「（当前）」", () => {
		expect(formatPeriodLabel("day", 0, NOW_MS)).toBe("2026年8月30日（当前）");
	});

	it("历史天不带标记", () => {
		expect(formatPeriodLabel("day", -1, NOW_MS)).toBe("2026年8月29日");
	});

	it("周标题带周数", () => {
		// 2026-08-24 周一所在周，2026-01-01 是周四。
		expect(formatPeriodLabel("week", 0, NOW_MS)).toBe("2026年第35周（当前）");
	});

	it("月标题", () => {
		expect(formatPeriodLabel("month", 0, NOW_MS)).toBe("2026年8月（当前）");
		expect(formatPeriodLabel("month", -1, NOW_MS)).toBe("2026年7月");
	});

	it("年标题", () => {
		expect(formatPeriodLabel("year", 0, NOW_MS)).toBe("2026年（当前）");
		expect(formatPeriodLabel("year", -1, NOW_MS)).toBe("2025年");
	});
});

describe("toLocalInputValue", () => {
	it("格式化为 datetime-local 字符串", () => {
		expect(toLocalInputValue(NOW_MS)).toBe("2026-08-30T15:30");
	});

	it("个位数补零", () => {
		const ms = new Date(2026, 0, 5, 8, 7).getTime();
		expect(toLocalInputValue(ms)).toBe("2026-01-05T08:07");
	});
});
