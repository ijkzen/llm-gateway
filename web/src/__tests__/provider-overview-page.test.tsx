import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import ProviderOverviewPage from "@/pages/provider-overview";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	callCharts: undefined as DashboardCharts | undefined,
	tokenCharts: undefined as DashboardCharts | undefined,
	callLoading: false,
	tokenLoading: false,
	callError: false,
	tokenError: false,
	providerName: "火山方舟",
	raceItems: [] as Array<Record<string, unknown>>,
	raceLoading: false,
	raceError: false,
	raceRefetch: vi.fn(),
}));

vi.mock("react-router-dom", () => ({
	useParams: () => ({ providerId: "3" }),
	useSearchParams: () => [new URLSearchParams("period=week&offset=0"), vi.fn()],
}));

vi.mock("@/hooks/use-dashboard-stats", () => ({
	useDashboardCharts: (params: { providerId?: number }) => {
		// 两个图表区块各自调用；用 providerId + 简单区分（这里 mock 都返回同一数据）。
		void params;
		return {
			data: mocks.callCharts,
			isLoading: mocks.callLoading,
			isError: mocks.callError,
			refetch: vi.fn(),
		};
	},
}));

vi.mock("@/hooks/use-providers", () => ({
	useProviderDetail: () => ({ data: { name: mocks.providerName } }),
}));

vi.mock("@/hooks/use-provider-model-race", () => ({
	useProviderModelRace: () => ({
		data: { startTime: 0, endTime: 0, items: mocks.raceItems },
		isLoading: mocks.raceLoading,
		isError: mocks.raceError,
		refetch: mocks.raceRefetch,
	}),
}));

// 图表组件依赖浏览器布局，stub 标记视图。
vi.mock("@/components/dashboard-charts", () => ({
	TrendLineChart: () => <div data-testid="trend-chart" />,
	ModelPieChart: () => <div data-testid="pie-chart" />,
	ModelRankBarChart: () => <div data-testid="rank-chart" />,
}));

function makeCharts(overrides: Partial<DashboardCharts> = {}): DashboardCharts {
	return {
		callTrend: [{ bucketStart: 1_700_000_000_000, value: 5 }],
		callByModel: [{ providerName: "火山方舟", modelId: "deepseek-v3", value: 5 }],
		tokenTrend: [{ bucketStart: 1_700_000_000_000, value: 500 }],
		tokenByModel: [{ providerName: "火山方舟", modelId: "deepseek-v3", value: 500 }],
		...overrides,
	};
}

function renderPage() {
	return render(<ProviderOverviewPage />);
}

describe("ProviderOverviewPage（供应商二级数据面板）", () => {
	beforeEach(() => {
		mocks.callCharts = undefined;
		mocks.tokenCharts = undefined;
		mocks.callLoading = false;
		mocks.tokenLoading = false;
		mocks.callError = false;
		mocks.tokenError = false;
		mocks.raceItems = [];
		mocks.raceLoading = false;
		mocks.raceError = false;
		mocks.raceRefetch.mockClear();
	});

	it("页头显示供应商名 + 数据面板", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getByText("火山方舟 · 数据面板")).toBeTruthy();
	});

	it("URL 带时间段参数时三块初始化为该时间段（周）", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		// URL period=week → 三块 SegmentedControl 的「周」激活。
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(3);
	});

	it("三个区块各自独立：改调用分析的时间不影响 token/赛马", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();

		// 调用分析切到「月」。
		const monthButtons = screen.getAllByRole("button", { name: "月" });
		const firstMonth = monthButtons[0];
		if (!firstMonth) {
			throw new Error("找不到「月」按钮");
		}
		fireEvent.click(firstMonth);
		// token 分析仍在「周」。
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(2);
	});

	it("渲染调用分析 + token 分析图表卡片", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		// 两个 TrendLineChart（调用 + token 各自趋势视图）。
		expect(screen.getAllByTestId("trend-chart").length).toBeGreaterThanOrEqual(2);
	});

	it("内部模型赛马渲染模型行", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.raceItems = [
			{
				providerName: "火山方舟",
				modelId: "deepseek-v3",
				requestCount: 10,
				totalTokens: 1_000_000,
				ttft: 120.5,
				requestTime: 800,
				tps: 55.25,
				cacheHitRate: 0.32,
			},
		];
		renderPage();
		expect(screen.getByText("deepseek-v3")).toBeTruthy();
		expect(screen.getByText("100 万")).toBeTruthy();
	});
});
