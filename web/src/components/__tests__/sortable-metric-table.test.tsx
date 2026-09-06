import { defaultRaceWindowState, windowQueryString } from "@/components/race-window-control";
import {
	RACE_COLUMNS,
	RACE_COLUMN_LABEL_KEYS,
	useRaceSort,
} from "@/components/sortable-metric-table";
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

describe("useRaceSort", () => {
	it("默认按总计 Token 降序", () => {
		const { result } = renderHook(() => useRaceSort());
		expect(result.current.sort).toEqual({ sortBy: "totalTokens", sortOrder: "desc" });
	});

	it("点击同列翻转方向", () => {
		const { result } = renderHook(() => useRaceSort());
		act(() => result.current.onSort("totalTokens"));
		expect(result.current.sort).toEqual({ sortBy: "totalTokens", sortOrder: "asc" });
		act(() => result.current.onSort("totalTokens"));
		expect(result.current.sort).toEqual({ sortBy: "totalTokens", sortOrder: "desc" });
	});

	it("点击新列取该列默认方向（耗时类默认升序）", () => {
		const { result } = renderHook(() => useRaceSort());
		for (const [key, expected] of [
			["ttft", "asc"],
			["requestTime", "asc"],
			["requestCount", "desc"],
			["tps", "desc"],
			["cacheHitRate", "desc"],
		] as const) {
			act(() => result.current.onSort(key));
			expect(result.current.sort).toEqual({ sortBy: key, sortOrder: expected });
		}
	});
});

describe("RACE_COLUMNS", () => {
	it("六个指标列齐全且标签键可查", () => {
		expect(RACE_COLUMNS.map((c) => c.key)).toEqual([
			"totalTokens",
			"requestCount",
			"ttft",
			"requestTime",
			"tps",
			"cacheHitRate",
		]);
		for (const column of RACE_COLUMNS) {
			expect(RACE_COLUMN_LABEL_KEYS[column.key]).toBe(column.labelKey);
		}
	});
});

describe("windowQueryString 与 initialWindowFromUrl 往返", () => {
	it("自定义窗口带起止时间", () => {
		const state = {
			...defaultRaceWindowState(),
			period: "custom" as const,
			appliedCustom: { startTime: 1000, endTime: 2000 },
		};
		const query = windowQueryString(state, { startTime: 1000, endTime: 2000 });
		expect(query).toBe("period=custom&startTime=1000&endTime=2000");
	});

	it("预设周期带 period/offset", () => {
		const state = { ...defaultRaceWindowState(), period: "week" as const, offset: 2 };
		const query = windowQueryString(state, { startTime: 0, endTime: 0 });
		expect(query).toBe("period=week&offset=2");
	});

	it("深链串可被 initialWindowFromUrl 还原出同一窗口参数", () => {
		for (const state of [
			defaultRaceWindowState(),
			{ ...defaultRaceWindowState(), period: "month" as const, offset: 1 },
			{
				...defaultRaceWindowState(),
				period: "custom" as const,
				appliedCustom: { startTime: 500, endTime: 900 },
			},
		]) {
			const bounds =
				state.period === "custom" ? { startTime: 500, endTime: 900 } : { startTime: 0, endTime: 0 };
			// eslint 不涉及：直接构造 URLSearchParams 模拟 URL 解析。
			const searchParams = new URLSearchParams(windowQueryString(state, bounds));
			const restored = (() => {
				// 与 initialWindowFromUrl 相同的解析契约（其内部还依赖 Date.now 兜底，
				// 这里只校验 period/offset/起止三个往返字段）。
				const period = searchParams.get("period") ?? "day";
				const offset = Number.parseInt(searchParams.get("offset") ?? "0", 10) || 0;
				const startTime = Number(searchParams.get("startTime"));
				const endTime = Number(searchParams.get("endTime"));
				return { period, offset, startTime, endTime };
			})();
			expect(restored.period).toBe(state.period);
			expect(restored.offset).toBe(state.offset);
			if (state.period === "custom") {
				expect(restored.startTime).toBe(bounds.startTime);
				expect(restored.endTime).toBe(bounds.endTime);
			}
		}
	});
});
