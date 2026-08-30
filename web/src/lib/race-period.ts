/**
 * 赛马时间窗口（天/周/月/年）的自然周期计算。
 *
 * 全部按浏览器本地时区解释，返回毫秒时间戳：
 * - 当前周期（offset=0）：[周期起点, now]，统计到当前时刻；
 * - 历史周期（offset<0）/未来周期（offset>0）：[周期起点, 下一周期起点) 半开区间。
 */

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

/** 中文月名。 */
const MONTH_NAMES = [
	"1月",
	"2月",
	"3月",
	"4月",
	"5月",
	"6月",
	"7月",
	"8月",
	"9月",
	"10月",
	"11月",
	"12月",
];

/**
 * 周期窗口的展示标题。
 * @param now 当前时刻（毫秒时间戳），用于「当前周期」标记。
 */
export function formatPeriodLabel(period: RacePeriod, offset: number, now: number): string {
	const bounds = periodBounds(period, offset, now);
	const start = new Date(bounds.startTime);
	const isCurrent = bounds.endTime === now;
	const currentSuffix = isCurrent ? "（当前）" : "";

	switch (period) {
		case "day":
			return `${start.getFullYear()}年${start.getMonth() + 1}月${start.getDate()}日${currentSuffix}`;
		case "week": {
			// 周数按 ISO 8601：周一为一周起点。
			const jan1 = new Date(start.getFullYear(), 0, 1);
			const weekNumber = Math.ceil(
				((start.getTime() - jan1.getTime()) / DAY_MS + jan1.getDay() + 1) / 7,
			);
			return `${start.getFullYear()}年第${weekNumber}周${currentSuffix}`;
		}
		case "month":
			return `${start.getFullYear()}年${MONTH_NAMES[start.getMonth()]}${currentSuffix}`;
		case "year":
			return `${start.getFullYear()}年${currentSuffix}`;
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
