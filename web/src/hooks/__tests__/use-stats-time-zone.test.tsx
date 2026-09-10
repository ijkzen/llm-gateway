import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	settings: undefined as Array<{ key: string; value: string }> | undefined,
}));

vi.mock("@/hooks/use-settings", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-settings")>("@/hooks/use-settings");
	return {
		...actual,
		useSettings: () => ({ data: mocks.settings, isLoading: false, isError: false }),
	};
});

// 19-22：setup.ts 对 use-stats-time-zone 做了全局 mock，这里取真实模块直测
// （真实实现覆盖「读 timezone 行 / 缺行与空值回退默认」两条逻辑）。
const { DEFAULT_STATS_TIME_ZONE, STATS_TIME_ZONE_KEY, useStatsTimeZone } = await vi.importActual<
	typeof import("@/hooks/use-stats-time-zone")
>("@/hooks/use-stats-time-zone");

function wrapper({ children }: { children: ReactNode }) {
	return <QueryClientProvider client={new QueryClient()}>{children}</QueryClientProvider>;
}

describe("useStatsTimeZone 真实实现（19-22）", () => {
	it("常量与后端口径一致", () => {
		expect(STATS_TIME_ZONE_KEY).toBe("timezone");
		expect(DEFAULT_STATS_TIME_ZONE).toBe("Asia/Shanghai");
	});

	it("读设置表的 timezone 行", () => {
		mocks.settings = [{ key: "timezone", value: "America/New_York" }];
		const { result } = renderHook(() => useStatsTimeZone(), { wrapper });
		expect(result.current).toBe("America/New_York");
	});

	it("设置表无 timezone 行时回退默认时区", () => {
		mocks.settings = [{ key: "language", value: "en" }];
		const { result } = renderHook(() => useStatsTimeZone(), { wrapper });
		expect(result.current).toBe(DEFAULT_STATS_TIME_ZONE);
	});

	it("timezone 行为空值（空串）时同样回退默认", () => {
		mocks.settings = [{ key: "timezone", value: "" }];
		const { result } = renderHook(() => useStatsTimeZone(), { wrapper });
		expect(result.current).toBe(DEFAULT_STATS_TIME_ZONE);
	});

	it("设置未加载（data 为 undefined）时回退默认", () => {
		mocks.settings = undefined;
		const { result } = renderHook(() => useStatsTimeZone(), { wrapper });
		expect(result.current).toBe(DEFAULT_STATS_TIME_ZONE);
	});
});
