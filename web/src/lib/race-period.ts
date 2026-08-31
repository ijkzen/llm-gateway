/**
 * 赛马时间窗口（天/周/月/年）的自然周期计算。
 *
 * 全部按浏览器本地时区解释，返回毫秒时间戳：
 * - 当前周期（offset=0）：[周期起点, now]，统计到当前时刻；
 * - 历史周期（offset<0）/未来周期（offset>0）：[周期起点, 下一周期起点) 半开区间。
 */

import i18n from "@/i18n";

export type RacePeriod = "day" | "week" | "month" | "year";

/** 图表桶粒度（透传给 /api/stats/charts 的 granularity 参数）。 */
export type ChartGranularity = "hour" | "day" | "month" | "year";

export interface PeriodBounds {
	/** 窗口起点（毫秒时间戳，含）。 */
	startTime: number;
	/** 窗口终点（毫秒时间戳，不含）。 */
	endTime: number;
}

/** 一天（本地时区）的毫秒数。 */
const DAY_MS = 24 * 60 * 60 * 1000;

function startOfDay(date: Date): Date {
	return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function startOfWeek(date: Date): Date {
	const day = date.getDay(); // 0=周日
	const diff = day === 0 ? -6 : 1 - day; // 周一为一周起点
	return startOfDay(new Date(date.getFullYear(), date.getMonth(), date.getDate() + diff));
}

function startOfMonth(date: Date): Date {
	return new Date(date.getFullYear(), date.getMonth(), 1);
}

function startOfYear(date: Date): Date {
	return new Date(date.getFullYear(), 0, 1);
}

/** 返回 offset 个周期偏移后的周期起点（基于 base 所在周期）。 */
function shiftPeriod(period: RacePeriod, base: Date, offset: number): Date {
	const start = periodStart(period, base);
	return new Date(
		period === "day"
			? start.getTime() + offset * DAY_MS
			: period === "week"
				? start.getTime() + offset * 7 * DAY_MS
				: period === "month"
					? new Date(start.getFullYear(), start.getMonth() + offset, 1)
					: new Date(start.getFullYear() + offset, 0, 1),
	);
}

/** 某周期起点的下周期起点（本地时区，处理月/年边界）。 */
function nextPeriodStart(period: RacePeriod, start: Date): Date {
	return new Date(
		period === "day"
			? start.getTime() + DAY_MS
			: period === "week"
				? start.getTime() + 7 * DAY_MS
				: period === "month"
					? new Date(start.getFullYear(), start.getMonth() + 1, 1)
					: new Date(start.getFullYear() + 1, 0, 1),
	);
}

function periodStart(period: RacePeriod, date: Date): Date {
	switch (period) {
		case "day":
			return startOfDay(date);
		case "week":
			return startOfWeek(date);
		case "month":
			return startOfMonth(date);
		case "year":
			return startOfYear(date);
	}
}

/**
 * 计算偏移后的周期窗口。
 * @param period 周期类型
 * @param offset 相对当前周期的偏移（0=当前，-1=上一周期，1=下一周期）
 * @param now 当前时刻（毫秒时间戳，测试可注入）
 */
export function periodBounds(period: RacePeriod, offset: number, now: number): PeriodBounds {
	const nowDate = new Date(now);
	const currentStart = periodStart(period, nowDate);
	const targetStart = shiftPeriod(period, nowDate, offset);
	const nextStart = nextPeriodStart(period, targetStart);

	if (targetStart.getTime() <= currentStart.getTime() && nextStart.getTime() > now) {
		// 当前周期（含偏移回退到当前）：终点截到 now。
		return { startTime: targetStart.getTime(), endTime: now };
	}
	// 历史或未来周期：完整半开区间。
	return { startTime: targetStart.getTime(), endTime: nextStart.getTime() };
}

/**
 * 周期窗口的展示标题。中文：`2026年8月（当前）`；英文：`Aug 2026 (current)`。
 * @param now 当前时刻（毫秒时间戳），用于「当前周期」标记。
 */
export function formatPeriodLabel(period: RacePeriod, offset: number, now: number): string {
	const bounds = periodBounds(period, offset, now);
	const start = new Date(bounds.startTime);
	const isCurrent = bounds.endTime === now;
	const zh = i18n.language.startsWith("zh");
	const currentSuffix = isCurrent ? (zh ? "（当前）" : " (current)") : "";

	switch (period) {
		case "day":
			return zh
				? `${start.getFullYear()}年${start.getMonth() + 1}月${start.getDate()}日${currentSuffix}`
				: `${start.toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" })}${currentSuffix}`;
		case "week": {
			// 周数按 ISO 8601：周一为一周起点。
			const weekNumber = isoWeekNumber(start);
			return zh
				? `${start.getFullYear()}年第${weekNumber}周${currentSuffix}`
				: `Week ${weekNumber}, ${start.getFullYear()}${currentSuffix}`;
		}
		case "month":
			return zh
				? `${start.getFullYear()}年${start.getMonth() + 1}月${currentSuffix}`
				: `${start.toLocaleDateString("en-US", { year: "numeric", month: "short" })}${currentSuffix}`;
		case "year":
			return zh
				? `${start.getFullYear()}年${currentSuffix}`
				: `${start.getFullYear()}${currentSuffix}`;
	}
}

/** 自定义时间输入框的本地时间字符串（datetime-local 格式 yyyy-MM-ddTHH:mm）。 */
export function toLocalInputValue(ms: number): string {
	const date = new Date(ms);
	const pad = (n: number) => n.toString().padStart(2, "0");
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/**
 * 由时间窗口推导图表桶粒度：
 * - 预设周期：天→小时桶、周/月→天桶、年→月桶；
 * - 自定义：时长 ≤24h→小时、≤31 天→天、≤366 天→月（自然月）、否则→年（自然年）。
 */
export function chartGranularity(
	period: RacePeriod | "custom",
	startTime: number,
	endTime: number,
): ChartGranularity {
	if (period !== "custom") {
		switch (period) {
			case "day":
				return "hour";
			case "week":
			case "month":
				return "day";
			case "year":
				return "month";
		}
	}
	const duration = endTime - startTime;
	const HOUR_MS = 3_600_000;
	const DAY_MS = 24 * HOUR_MS;
	if (duration <= 24 * HOUR_MS) {
		return "hour";
	}
	if (duration <= 31 * DAY_MS) {
		return "day";
	}
	if (duration <= 366 * DAY_MS) {
		return "month";
	}
	return "year";
}

/** ISO 周数（周一为一周起点，与 startOfWeek 口径一致）。 */
function isoWeekNumber(date: Date): number {
	const jan1 = new Date(date.getFullYear(), 0, 1);
	return Math.ceil(((date.getTime() - jan1.getTime()) / DAY_MS + jan1.getDay() + 1) / 7);
}

/** 两位补零。 */
function pad2(n: number): string {
	return n.toString().padStart(2, "0");
}

/**
 * 周期窗口的紧凑标题（不带「当前」标记）：
 * 天 → 2026/08/31；周 → 2026-36W；月 → 2026/08；年 → 2026。
 */
export function formatCompactPeriodLabel(period: RacePeriod, offset: number, now: number): string {
	const bounds = periodBounds(period, offset, now);
	const start = new Date(bounds.startTime);
	switch (period) {
		case "day":
			return `${start.getFullYear()}/${pad2(start.getMonth() + 1)}/${pad2(start.getDate())}`;
		case "week":
			return `${start.getFullYear()}-${isoWeekNumber(start)}W`;
		case "month":
			return `${start.getFullYear()}/${pad2(start.getMonth() + 1)}`;
		case "year":
			return `${start.getFullYear()}`;
	}
}

/**
 * 自定义时间单行展示：`2026/08/31 24:00:00`。
 * 次日 0 点显示为前一日 24:00:00（跨日 0 点语义），其余按 yyyy/MM/dd HH:mm:ss。
 */
export function formatDateTimeLabel(ms: number): string {
	const date = new Date(ms);
	const isMidnight = date.getHours() === 0 && date.getMinutes() === 0 && date.getSeconds() === 0;
	if (isMidnight) {
		const prev = new Date(ms - 1);
		return `${prev.getFullYear()}/${pad2(prev.getMonth() + 1)}/${pad2(prev.getDate())} 24:00:00`;
	}
	return `${date.getFullYear()}/${pad2(date.getMonth() + 1)}/${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`;
}

/** 自定义窗口默认值：开始 = 7 天前 0 点，结束 = 明天 0 点（覆盖「过去 7 天含今天」）。 */
export function defaultCustomWindow(now: number): { startTime: number; endTime: number } {
	const start = new Date(now);
	start.setDate(start.getDate() - 7);
	start.setHours(0, 0, 0, 0);
	const end = new Date(now);
	end.setDate(end.getDate() + 1);
	end.setHours(0, 0, 0, 0);
	return { startTime: start.getTime(), endTime: end.getTime() };
}
