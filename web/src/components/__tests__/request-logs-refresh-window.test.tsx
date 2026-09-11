import { PageRefreshButton } from "@/components/page-refresh-button";
import { RequestLogsTable } from "@/components/request-logs/RequestLogsTable";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * 回归：请求日志页的时间窗终点必须随重取前进。
 *
 * 此前 `endTime` 由挂载时刻固化的 now 派生并写进 query key，顶栏刷新只是拿同一
 * 个 key 重跑 queryFn，于是复用旧窗口——打开页面之后产生的日志刷新多少次都
 * 看不到。修法是把绝对起止移出 key、延到 queryFn 取数时解析。
 *
 * 本文件用真实 useRequestLogs + 真实 QueryClient，只在 api 层拦截并记录请求
 * URL，从而断言「点刷新 → 发出的 endTime 更新」这一端到端行为。
 */
const mocks = vi.hoisted(() => ({ requests: [] as string[] }));

vi.mock("@/hooks/use-virtual-models", () => ({ useVirtualModels: () => ({ data: [] }) }));
vi.mock("@/hooks/use-api-keys", () => ({ useApiKeys: () => ({ data: [] }) }));
vi.mock("@/hooks/use-providers", () => ({
	useProviders: () => ({ data: [] }),
	useProviderDetail: () => ({ data: undefined }),
}));
vi.mock("@/hooks/use-provider-models", () => ({ useProviderModels: () => ({ data: [] }) }));

vi.mock("@/lib/api", async () => {
	const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
	return {
		...actual,
		api: {
			get: (url: string) => {
				mocks.requests.push(url);
				return {
					json: async () => ({
						code: "0",
						msg: "ok",
						data: { items: [], total: 0, page: 1, pageSize: 20 },
					}),
				};
			},
		},
	};
});

function endTimeOf(url: string): number {
	return Number(new URLSearchParams(url.split("?")[1] ?? "").get("endTime"));
}

function startTimeOf(url: string): number {
	return Number(new URLSearchParams(url.split("?")[1] ?? "").get("startTime"));
}

function renderPage() {
	const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
	return render(
		<QueryClientProvider client={client}>
			<PageRefreshButton />
			<RequestLogsTable />
		</QueryClientProvider>,
	);
}

describe("请求日志时间窗随刷新前进（回归）", () => {
	beforeEach(() => {
		mocks.requests.length = 0;
		window.localStorage.clear();
	});

	it("点顶栏刷新后 endTime 前进到当前时刻", async () => {
		vi.useFakeTimers({ shouldAdvanceTime: true });
		vi.setSystemTime(new Date(2026, 7, 31, 10, 0, 0));
		renderPage();

		await waitFor(() => expect(mocks.requests.length).toBeGreaterThan(0));
		const firstEnd = endTimeOf(mocks.requests[0] ?? "");
		expect(firstEnd).toBe(new Date(2026, 7, 31, 10, 0, 0).getTime());

		// 期间产生了新日志（时间前进），刷新应把终点推到此刻。
		vi.setSystemTime(new Date(2026, 7, 31, 10, 5, 0));
		fireEvent.click(screen.getByRole("button", { name: "刷新" }));
		await waitFor(() => expect(mocks.requests.length).toBeGreaterThan(1));

		const lastEnd = endTimeOf(mocks.requests[mocks.requests.length - 1] ?? "");
		expect(lastEnd).toBe(new Date(2026, 7, 31, 10, 5, 0).getTime());
		expect(lastEnd).toBeGreaterThan(firstEnd);

		vi.useRealTimers();
	});

	it("跨零点后刷新，窗口起点滚到新的一天而非停在昨天", async () => {
		vi.useFakeTimers({ shouldAdvanceTime: true });
		vi.setSystemTime(new Date(2026, 7, 31, 23, 50, 0));
		renderPage();

		await waitFor(() => expect(mocks.requests.length).toBeGreaterThan(0));
		expect(new Date(startTimeOf(mocks.requests[0] ?? "")).getDate()).toBe(31);

		vi.setSystemTime(new Date(2026, 8, 1, 0, 10, 0));
		fireEvent.click(screen.getByRole("button", { name: "刷新" }));
		await waitFor(() => expect(mocks.requests.length).toBeGreaterThan(1));

		const last = mocks.requests[mocks.requests.length - 1] ?? "";
		expect(new Date(startTimeOf(last)).getDate()).toBe(1);
		expect(new Date(startTimeOf(last)).getHours()).toBe(0);
		expect(endTimeOf(last)).toBe(new Date(2026, 8, 1, 0, 10, 0).getTime());

		vi.useRealTimers();
	});
});
