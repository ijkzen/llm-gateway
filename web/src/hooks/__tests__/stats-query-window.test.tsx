import { statsQuery } from "@/hooks/stats-query";
import { type RaceWindowState, queryWindow } from "@/lib/race-period";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/**
 * 取数收敛点的时间窗语义回归：query key 只含窗口定义，绝对起止在 queryFn 内
 * 按调用时刻现算。此前绝对起止直接进 key 与参数，重取（刷新/聚焦/重试）复用
 * 旧窗口，当前周期永远截在挂载时刻。
 */
const mocks = vi.hoisted(() => ({ urls: [] as string[] }));

vi.mock("@/lib/api", async () => {
	const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
	return {
		...actual,
		api: {
			get: (url: string) => {
				mocks.urls.push(url);
				return { json: async () => ({ code: "0", msg: "ok", data: { ok: true } }) };
			},
		},
	};
});

const DAY: RaceWindowState = {
	period: "day",
	offset: 0,
	customStart: 0,
	customEnd: 0,
	appliedCustom: null,
};

function endTimeOf(url: string): number {
	return Number(new URLSearchParams(url.split("?")[1] ?? "").get("endTime"));
}

function wrapper(client: QueryClient) {
	return ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={client}>{children}</QueryClientProvider>
	);
}

describe("statsQuery 取数窗口", () => {
	beforeEach(() => {
		mocks.urls.length = 0;
		vi.useFakeTimers({ shouldAdvanceTime: true });
	});
	afterEach(() => {
		vi.useRealTimers();
	});

	it("窗口绝对起止拼进请求 URL，而非由调用方传入", async () => {
		vi.setSystemTime(new Date(2026, 7, 31, 10, 0, 0));
		const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
		renderHook(
			() =>
				statsQuery({
					endpoint: "stats/summary",
					key: ["stats", "summary", "test"],
					params: {},
					window: queryWindow(DAY, "Asia/Shanghai"),
				}),
			{ wrapper: wrapper(client) },
		);

		await waitFor(() => expect(mocks.urls.length).toBeGreaterThan(0));
		expect(mocks.urls[0]).toContain("startTime=");
		expect(endTimeOf(mocks.urls[0] ?? "")).toBe(new Date(2026, 7, 31, 10, 0, 0).getTime());
	});

	it("重取沿用同一 key 但换用当前时刻的窗口终点", async () => {
		vi.setSystemTime(new Date(2026, 7, 31, 10, 0, 0));
		const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
		const { result } = renderHook(
			() =>
				statsQuery({
					endpoint: "stats/summary",
					key: ["stats", "summary", "test"],
					params: {},
					window: queryWindow(DAY, "Asia/Shanghai"),
				}),
			{ wrapper: wrapper(client) },
		);

		await waitFor(() => expect(mocks.urls.length).toBe(1));
		expect(endTimeOf(mocks.urls[0] ?? "")).toBe(new Date(2026, 7, 31, 10, 0, 0).getTime());

		// 时间前进后重取：窗口终点前进（这正是「刷新看到最新数据」的前提）。
		vi.setSystemTime(new Date(2026, 7, 31, 10, 20, 0));
		await result.current.refetch();
		await waitFor(() => expect(mocks.urls.length).toBe(2));
		expect(endTimeOf(mocks.urls[1] ?? "")).toBe(new Date(2026, 7, 31, 10, 20, 0).getTime());
	});

	it("无窗口端点不注入时间参数（全历史累计）", async () => {
		const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
		renderHook(
			() =>
				statsQuery({
					endpoint: "stats/summary",
					key: ["stats", "summary", "all"],
					params: {},
				}),
			{ wrapper: wrapper(client) },
		);

		await waitFor(() => expect(mocks.urls.length).toBeGreaterThan(0));
		expect(mocks.urls[0]).toBe("stats/summary");
	});
});
