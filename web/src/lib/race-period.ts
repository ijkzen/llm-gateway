/**
 * 赛马时间窗口（天/周/月/年）的自然周期计算。
 *
 * 默认按浏览器本地时区解释；传入 IANA `timeZone`（如设置表时区 Asia/Shanghai）
 * 时按该时区解释（管理后台数据面板统一走设置表时区，与后端分桶口径一致）。
 * 返回毫秒时间戳：
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

/**
 * 时间窗口定义：预设周期（天/周/月/年 + 相对偏移）或自定义区间。
 * 绝对起止由 `now` 解析——同一个定义在不同时刻解析出不同终点。
 */
export interface RaceWindowState {
	period: RacePeriod | "custom";
	offset: number;
	customStart: number;
	customEnd: number;
	/** 已应用的自定义窗口（null 时退化为输入值）。 */
	appliedCustom: { startTime: number; endTime: number } | null;
}

/**
 * 取数窗口：key 用稳定身份（窗口定义 + 时区），绝对起止延到取数时现算。
 *
 * 存在的理由：把「挂载时刻解析出的绝对 endTime」放进 query key，会让重取复用
 * 旧 key 与旧窗口——当前周期被永久截在上一刻，刷新拿不到新数据。这里把身份与
 * 取值分开：key 只随定义变化，`resolve()` 每次取数都用调用时刻。
 */
export interface QueryWindow {
	/** query key 用的稳定身份（随窗口定义/时区变化，不含 now）。 */
	readonly key: readonly (string | number | null)[];
	/** 解析为绝对起止（当前周期截到调用时刻）。 */
	readonly resolve: () => PeriodBounds;
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

/** 时间窗口终局计算（三种调用路径共用）：当前周期截到 now，其余取完整半开区间。 */
function boundsOf(
	currentStart: number,
	targetStart: number,
	nextStart: number,
	now: number,
): PeriodBounds {
	if (targetStart <= currentStart && nextStart > now) {
		return { startTime: targetStart, endTime: now };
	}
	return { startTime: targetStart, endTime: nextStart };
}

// ── IANA 时区感知内核（Intl 墙钟部件 + 固定偏移模型，与后端口径一致） ──

const tzFormatters = new Map<string, Intl.DateTimeFormat>();

interface WallParts {
	y: number;
	m: number;
	d: number;
	hh: number;
	mm: number;
	ss: number;
}

function tzFormatter(tz: string): Intl.DateTimeFormat {
	let formatter = tzFormatters.get(tz);
	if (!formatter) {
		formatter = new Intl.DateTimeFormat("en-US", {
			timeZone: tz,
			year: "numeric",
			month: "2-digit",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
			second: "2-digit",
			hour12: false,
			hourCycle: "h23",
		});
		tzFormatters.set(tz, formatter);
	}
	return formatter;
}

/** 某时刻在该时区的墙钟部件。 */
function wallParts(tz: string, ms: number): WallParts {
	const parts = tzFormatter(tz).formatToParts(new Date(ms));
	const num = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? "0");
	return {
		y: num("year"),
		m: num("month"),
		d: num("day"),
		hh: num("hour"),
		mm: num("minute"),
		ss: num("second"),
	};
}

/** 某时刻该时区的 UTC 偏移（毫秒，东为正）：墙钟按 UTC 组装再与真实时刻相减。 */
function tzOffsetMs(tz: string, ms: number): number {
	const p = wallParts(tz, ms);
	return Date.UTC(p.y, p.m - 1, p.d, p.hh, p.mm, p.ss) - ms;
}

const wallKey = (p: Pick<WallParts, "y" | "m" | "d">) => p.y * 10000 + p.m * 100 + p.d;

/** 目标墙钟日（y/m/d）在指定时区的 0 点毫秒；DST 跳日/歧义时最多校正两次。 */
function wallDayStart(tz: string, y: number, m: number, d: number): number {
	const target = y * 10000 + m * 100 + d;
	let start = Date.UTC(y, m - 1, d) - tzOffsetMs(tz, Date.UTC(y, m - 1, d));
	for (let i = 0; i < 2; i++) {
		const p = wallParts(tz, start);
		if (wallKey(p) === target) {
			return start;
		}
		start += wallKey(p) < target ? DAY_MS : -DAY_MS;
	}
	return start;
}

/** 墙钟日加 days 天后的该时区 0 点（Date.UTC 自动处理月/年进位）。 */
function wallDayAdd(tz: string, p: WallParts, days: number): number {
	return wallDayStart(tz, p.y, p.m, p.d + days);
}

/** 墙钟月加 months 月后的该时区 1 日 0 点。 */
function wallMonthAdd(tz: string, p: WallParts, months: number): number {
	const total = p.y * 12 + (p.m - 1) + months;
	const y = Math.floor(total / 12);
	return wallDayStart(tz, y, total - y * 12 + 1, 1);
}

/** 某周期起点（指定时区）。周起点为周一；月/年为 1 日/1 月 1 日。 */
function periodStartInTz(period: RacePeriod, ms: number, tz: string): number {
	const p = wallParts(tz, ms);
	switch (period) {
		case "day":
			return wallDayStart(tz, p.y, p.m, p.d);
		case "week": {
			const mondayOffset = (new Date(Date.UTC(p.y, p.m - 1, p.d)).getUTCDay() + 6) % 7;
			return wallDayStart(tz, p.y, p.m, p.d - mondayOffset);
		}
		case "month":
			return wallDayStart(tz, p.y, p.m, 1);
		case "year":
			return wallDayStart(tz, p.y, 1, 1);
	}
}

/** 周期起点偏移 offset 个周期（指定时区，基于 base 所在周期）。 */
function shiftPeriodInTz(
	period: RacePeriod,
	baseStart: number,
	offset: number,
	tz: string,
): number {
	const p = wallParts(tz, baseStart);
	switch (period) {
		case "day":
			return wallDayAdd(tz, p, offset);
		case "week":
			return wallDayAdd(tz, p, offset * 7);
		case "month":
			return wallMonthAdd(tz, p, offset);
		case "year":
			return wallDayStart(tz, p.y + offset, 1, 1);
	}
}

/** 下周期起点（指定时区，处理月/年边界）。 */
function nextPeriodStartInTz(period: RacePeriod, start: number, tz: string): number {
	return shiftPeriodInTz(period, start, 1, tz);
}

/**
 * 计算偏移后的周期窗口。
 * @param period 周期类型
 * @param offset 相对当前周期的偏移（0=当前，-1=上一周期，1=下一周期）
 * @param now 当前时刻（毫秒时间戳，测试可注入）
 * @param timeZone 可选 IANA 时区；缺省按浏览器本地时区解释
 */
export function periodBounds(
	period: RacePeriod,
	offset: number,
	now: number,
	timeZone?: string,
): PeriodBounds {
	if (timeZone) {
		const currentStart = periodStartInTz(period, now, timeZone);
		const targetStart = shiftPeriodInTz(period, currentStart, offset, timeZone);
		const nextStart = nextPeriodStartInTz(period, targetStart, timeZone);
		return boundsOf(currentStart, targetStart, nextStart, now);
	}
	const nowDate = new Date(now);
	const currentStart = periodStart(period, nowDate);
	const targetStart = shiftPeriod(period, nowDate, offset);
	const nextStart = nextPeriodStart(period, targetStart);
	return boundsOf(currentStart.getTime(), targetStart.getTime(), nextStart.getTime(), now);
}

/** 由窗口状态派生绝对起止（自定义取已应用区间，预设周期按 now 解析）。 */
export function raceWindowBounds(
	state: RaceWindowState,
	now: number,
	timeZone?: string,
): PeriodBounds {
	if (state.period === "custom") {
		return state.appliedCustom ?? { startTime: state.customStart, endTime: state.customEnd };
	}
	return periodBounds(state.period, state.offset, now, timeZone);
}

/**
 * 构造取数窗口：key 用窗口定义 + 时区（跨渲染稳定），绝对起止延到取数时解析。
 * 自定义窗口的起止本身是稳定的，故直接进 key。
 */
export function queryWindow(state: RaceWindowState, timeZone?: string): QueryWindow {
	const custom = state.appliedCustom ?? { startTime: state.customStart, endTime: state.customEnd };
	return {
		key:
			state.period === "custom"
				? ["custom", custom.startTime, custom.endTime, timeZone ?? null]
				: [state.period, state.offset, timeZone ?? null],
		resolve: () => raceWindowBounds(state, Date.now(), timeZone),
	};
}

/**
 * 周期窗口的展示标题。中文：`2026年8月（当前）`；英文：`Aug 2026 (current)`。
 * @param now 当前时刻（毫秒时间戳），用于「当前周期」标记。
 * @param timeZone 可选 IANA 时区；缺省按浏览器本地时区解释。
 */
export function formatPeriodLabel(
	period: RacePeriod,
	offset: number,
	now: number,
	locale: "zh" | "en",
	timeZone?: string,
): string {
	const bounds = periodBounds(period, offset, now, timeZone);
	const startMs = bounds.startTime;
	const isCurrent = bounds.endTime === now;
	// 20-04：中文文案统一从 locales 取（time.*），不再在代码里手写；
	// 英文侧保留各自 Intl/手写格式（与既有断言一致）。
	const tr = (key: string, opts: Record<string, unknown>) =>
		i18n.getFixedT(null, "translation")(key, opts);
	const currentSuffix = isCurrent
		? locale === "zh"
			? tr("time.currentSuffix", {})
			: " (current)"
		: "";
	const start = new Date(startMs);

	if (timeZone) {
		const p = wallParts(timeZone, startMs);
		switch (period) {
			case "day":
				return locale === "zh"
					? `${tr("time.yearMonth", { year: p.y, month: p.m }).replace(`${p.m}月`, `${p.m}月${p.d}日`)}${currentSuffix}`
					: `${new Intl.DateTimeFormat("en-US", { timeZone, month: "short", day: "numeric" }).format(start)}${currentSuffix}`;
			case "week":
				return locale === "zh"
					? `${tr("time.yearWeek", { year: p.y, week: isoWeekNumberInTz(timeZone, startMs) })}${currentSuffix}`
					: `Week ${isoWeekNumberInTz(timeZone, startMs)}, ${p.y}${currentSuffix}`;
			case "month":
				return locale === "zh"
					? `${tr("time.yearMonth", { year: p.y, month: p.m })}${currentSuffix}`
					: `${monthNameInTz(timeZone, startMs)} ${p.y}${currentSuffix}`;
			case "year":
				return locale === "zh"
					? `${tr("time.year", { year: p.y })}${currentSuffix}`
					: `${p.y}${currentSuffix}`;
		}
	}

	switch (period) {
		case "day": {
			const month = start.getMonth() + 1;
			const tr = i18n.getFixedT(null, "translation");
			return locale === "zh"
				? `${tr("time.monthDay", { month, day: start.getDate() }).replace(`${month}月`, `${start.getFullYear()}年${month}月`)}${currentSuffix}`
				: `${start.toLocaleDateString("en-US", { year: "numeric", month: "short", day: "numeric" })}${currentSuffix}`;
		}
		case "week": {
			// 周数按 ISO 8601：周一为一周起点。
			const weekNumber = isoWeekNumber(start);
			return locale === "zh"
				? `${i18n.getFixedT(null, "translation")("time.yearWeek", { year: start.getFullYear(), week: weekNumber })}${currentSuffix}`
				: `Week ${weekNumber}, ${start.getFullYear()}${currentSuffix}`;
		}
		case "month":
			return locale === "zh"
				? `${i18n.getFixedT(null, "translation")("time.yearMonth", { year: start.getFullYear(), month: start.getMonth() + 1 })}${currentSuffix}`
				: `${start.toLocaleDateString("en-US", { year: "numeric", month: "short" })}${currentSuffix}`;
		case "year":
			return locale === "zh"
				? `${i18n.getFixedT(null, "translation")("time.year", { year: start.getFullYear() })}${currentSuffix}`
				: `${start.getFullYear()}${currentSuffix}`;
	}
}

/** 自定义时间输入框的本地时间字符串（datetime-local 格式 yyyy-MM-ddTHH:mm）。
 *  固定按浏览器本地时区解释：`<input type="datetime-local">` 原生只认本地墙钟值，
 *  无法承载设置表时区（16-08）；窗口边界本身已由 `defaultCustomWindow(tz)` 对齐。 */
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

/** ISO 周数（指定时区：按该时区墙钟年的 1 月 1 日计）。 */
function isoWeekNumberInTz(tz: string, startMs: number): number {
	const p = wallParts(tz, startMs);
	const jan1 = wallDayStart(tz, p.y, 1, 1);
	const jan1Weekday = new Date(Date.UTC(p.y, 0, 1)).getUTCDay();
	return Math.ceil((startMs - jan1) / DAY_MS + jan1Weekday + 1);
}

/** 英文月份短名（指定时区，如 Aug）。 */
function monthNameInTz(tz: string, ms: number): string {
	return new Intl.DateTimeFormat("en-US", { timeZone: tz, month: "short" }).format(new Date(ms));
}

/** 两位补零。 */
function pad2(n: number): string {
	return n.toString().padStart(2, "0");
}

/**
 * 周期窗口的紧凑标题（不带「当前」标记）：
 * 天 → 2026/08/31；周 → 2026-36W；月 → 2026/08；年 → 2026。
 * @param timeZone 可选 IANA 时区；缺省按浏览器本地时区解释。
 */
export function formatCompactPeriodLabel(
	period: RacePeriod,
	offset: number,
	now: number,
	timeZone?: string,
): string {
	const bounds = periodBounds(period, offset, now, timeZone);
	const startMs = bounds.startTime;
	if (timeZone) {
		const p = wallParts(timeZone, startMs);
		switch (period) {
			case "day":
				return `${p.y}/${pad2(p.m)}/${pad2(p.d)}`;
			case "week":
				return `${p.y}-${isoWeekNumberInTz(timeZone, startMs)}W`;
			case "month":
				return `${p.y}/${pad2(p.m)}`;
			case "year":
				return `${p.y}`;
		}
	}
	const start = new Date(startMs);
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

/**
 * 自定义窗口默认值：开始 = 7 天前 0 点，结束 = 明天 0 点（覆盖「过去 7 天含今天」）。
 *
 * `timeZone` 为设置表口径时区（16-08）：预设周期的取整走该时区，自定义窗口也
 * 必须一致，否则浏览器时区与设置表不同时两者整体偏移、与后端分桶不对齐。
 * 缺省用浏览器本地时区（无设置来源的调用场景）。
 */
export function defaultCustomWindow(
	now: number,
	timeZone?: string,
): { startTime: number; endTime: number } {
	if (timeZone) {
		const p = wallParts(timeZone, now);
		const start = wallDayStart(timeZone, p.y, p.m, p.d - 7);
		const end = wallDayStart(timeZone, p.y, p.m, p.d + 1);
		return { startTime: start, endTime: end };
	}
	const start = new Date(now);
	start.setDate(start.getDate() - 7);
	start.setHours(0, 0, 0, 0);
	const end = new Date(now);
	end.setDate(end.getDate() + 1);
	end.setHours(0, 0, 0, 0);
	return { startTime: start.getTime(), endTime: end.getTime() };
}
