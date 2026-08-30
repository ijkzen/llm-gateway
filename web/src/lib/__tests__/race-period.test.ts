import {
	chartGranularity,
	defaultCustomWindow,
	formatCompactPeriodLabel,
	formatDateTimeLabel,
	formatPeriodLabel,
	periodBounds,
	toLocalInputValue,
} from "@/lib/race-period";
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

describe("chartGranularity 桶粒度推导", () => {
	const H = 3_600_000;
	const D = 24 * H;
	const t0 = new Date(2026, 7, 30, 0, 0).getTime();

	it("预设周期：天→hour、周/月→day、年→month", () => {
		expect(chartGranularity("day", t0, t0 + 5 * H)).toBe("hour");
		expect(chartGranularity("week", t0, t0 + 7 * D)).toBe("day");
		expect(chartGranularity("month", t0, t0 + 31 * D)).toBe("day");
		expect(chartGranularity("year", t0, t0 + 366 * D)).toBe("month");
	});

	it("自定义 ≤24h → hour", () => {
		expect(chartGranularity("custom", t0, t0 + 5 * H)).toBe("hour");
		expect(chartGranularity("custom", t0, t0 + 24 * H)).toBe("hour");
	});

	it("自定义 24h~31 天 → day", () => {
		expect(chartGranularity("custom", t0, t0 + 25 * H)).toBe("day");
		expect(chartGranularity("custom", t0, t0 + 31 * D)).toBe("day");
	});

	it("自定义 >31 天且 ≤366 天 → month（跨年短区间也按月）", () => {
		expect(chartGranularity("custom", t0, t0 + 63 * D)).toBe("month");
		// 12/20 ~ 1/15（约 27 天，但跨年）：按时长仍 ≤31 天 → day。
		const dec = new Date(2026, 11, 20).getTime();
		const jan = new Date(2027, 0, 15).getTime();
		expect(chartGranularity("custom", dec, jan)).toBe("day");
		// 跨年且超 31 天 → month。
		expect(chartGranularity("custom", dec, new Date(2027, 1, 15).getTime())).toBe("month");
	});

	it("自定义 >366 天 → year", () => {
		expect(chartGranularity("custom", t0, t0 + 367 * D)).toBe("year");
		expect(chartGranularity("custom", t0, t0 + 2 * 366 * D)).toBe("year");
	});
});

describe("formatCompactPeriodLabel 紧凑周期标题", () => {
	// 锚点：2026-08-30（周日）15:30 本地；周一起点为 08-24。
	it("天 → 2026/08/31", () => {
		expect(formatCompactPeriodLabel("day", 0, NOW_MS)).toBe("2026/08/30");
		expect(formatCompactPeriodLabel("day", -1, NOW_MS)).toBe("2026/08/29");
		expect(formatCompactPeriodLabel("day", 1, NOW_MS)).toBe("2026/08/31");
	});

	it("周 → 2026-36W（当前周）", () => {
		// 2026-08-24 周一起点所在周 = ISO 第 35 周（formatPeriodLabel 已有断言）。
		expect(formatCompactPeriodLabel("week", 0, NOW_MS)).toBe("2026-35W");
		expect(formatCompactPeriodLabel("week", -1, NOW_MS)).toBe("2026-34W");
	});

	it("月 → 2026/08", () => {
		expect(formatCompactPeriodLabel("month", 0, NOW_MS)).toBe("2026/08");
		expect(formatCompactPeriodLabel("month", -1, NOW_MS)).toBe("2026/07");
	});

	it("年 → 2026", () => {
		expect(formatCompactPeriodLabel("year", 0, NOW_MS)).toBe("2026");
		expect(formatCompactPeriodLabel("year", -1, NOW_MS)).toBe("2025");
	});
});

describe("formatDateTimeLabel 自定义单行格式", () => {
	it("普通时刻 → yyyy/MM/dd HH:mm:ss", () => {
		const ms = new Date(2026, 7, 31, 14, 5, 9).getTime();
		expect(formatDateTimeLabel(ms)).toBe("2026/08/31 14:05:09");
	});

	it("整点补零", () => {
		const ms = new Date(2026, 7, 31, 0, 0, 0).getTime();
		expect(formatDateTimeLabel(ms)).toBe("2026/08/30 24:00:00");
	});

	it("次日 0 点 → 前一日 24:00:00", () => {
		const ms = new Date(2026, 8, 1, 0, 0, 0).getTime();
		expect(formatDateTimeLabel(ms)).toBe("2026/08/31 24:00:00");
	});

	it("月初 0 点 → 上月末 24:00:00", () => {
		const ms = new Date(2026, 8, 1, 0, 0, 0).getTime();
		expect(formatDateTimeLabel(ms)).toBe("2026/08/31 24:00:00");
	});
});

describe("defaultCustomWindow 自定义默认窗口", () => {
	it("开始 = 7 天前 0 点，结束 = 明天 0 点", () => {
		const { startTime, endTime } = defaultCustomWindow(NOW_MS);
		// 2026-08-30 15:30 → 开始 2026-08-23 00:00，结束 2026-08-31 00:00。
		expect(new Date(startTime).getHours()).toBe(0);
		expect(new Date(startTime).getDate()).toBe(23);
		expect(new Date(endTime).getHours()).toBe(0);
		expect(new Date(endTime).getDate()).toBe(31);
		expect(endTime - startTime).toBe(8 * 24 * 60 * 60 * 1000);
	});
});
