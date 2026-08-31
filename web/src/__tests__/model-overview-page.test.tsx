import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import ModelOverviewPage from "@/pages/model-overview";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	callCharts: undefined as DashboardCharts | undefined,
	tokenCharts: undefined as DashboardCharts | undefined,
	callLoading: false,
	tokenLoading: false,
	metrics: undefined as Record<string, unknown> | undefined,
	metricsLoading: false,
	metricsError: false,
}));

vi.mock("react-router-dom", () => ({
	useParams: () => ({ providerId: "3", modelId: "deepseek-v3" }),
	useSearchParams: () => [new URLSearchParams("period=week&offset=0"), vi.fn()],
}));

vi.mock("@/hooks/use-dashboard-stats", () => ({
	useDashboardCharts: () => ({
		data: mocks.callCharts,
		isLoading: mocks.callLoading,
		isError: false,
		refetch: vi.fn(),
	}),
}));

vi.mock("@/hooks/use-dashboard-insight", () => ({
	useDashboardInsight: () => ({
		data: {
			failureTrend: [],
			failureRateTrend: [],
			failureReasons: [],
			ttftPercentiles: [],
			latencyPercentiles: [],
			inputTokenTrend: [],
			outputTokenTrend: [],
			cacheHitRateTrend: [],
			outputTokensPerSecTrend: [],
			apiKeyRank: [],
			rpmTrend: [],
			tpmTrend: [],
			streamRatioTrend: [],
		},
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));

vi.mock("@/hooks/use-model-metrics", () => ({
	useModelMetrics: () => ({
		data: mocks.metrics,
		isLoading: mocks.metricsLoading,
		isError: mocks.metricsError,
		refetch: vi.fn(),
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
		callByModel: [],
		tokenTrend: [{ bucketStart: 1_700_000_000_000, value: 500 }],
		tokenByModel: [],
		...overrides,
	};
}

function renderPage() {
	return render(<ModelOverviewPage />);
}

describe("ModelOverviewPage（模型详情三级页）", () => {
	beforeEach(() => {
		mocks.callCharts = undefined;
		mocks.tokenCharts = undefined;
		mocks.callLoading = false;
		mocks.tokenLoading = false;
		mocks.metrics = undefined;
		mocks.metricsLoading = false;
		mocks.metricsError = false;
	});

	it("页头显示供应商・模型", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.metrics = {
			providerName: "火山方舟",
			modelId: "deepseek-v3",
			totalTokens: 100,
			requestCount: 1,
			ttft: 100,
			requestTime: 500,
			tps: 10,
			cacheHitRate: 0.1,
		};
		renderPage();
		expect(screen.getByText("火山方舟・deepseek-v3 · 模型数据")).toBeTruthy();
	});

	it("URL 带时间段参数时三块初始化为该时间段（周）", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.metrics = {
			providerName: "火山方舟",
			totalTokens: 100,
			requestCount: 1,
			ttft: 100,
			requestTime: 500,
			tps: 10,
			cacheHitRate: 0.1,
		};
		renderPage();
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(3);
	});

	it("三个区块各自独立：改调用分析的时间不影响其他", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.metrics = {
			providerName: "火山方舟",
			totalTokens: 100,
			requestCount: 1,
			ttft: 100,
			requestTime: 500,
			tps: 10,
			cacheHitRate: 0.1,
		};
		renderPage();

		const monthButtons = screen.getAllByRole("button", { name: "月" });
		const firstMonth = monthButtons[0];
		if (!firstMonth) {
			throw new Error("找不到「月」按钮");
		}
		fireEvent.click(firstMonth);
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(2);
	});

	it("渲染两个折线图（调用 + Token），无饼图/条形图", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.metrics = {
			providerName: "火山方舟",
			totalTokens: 100,
			requestCount: 1,
			ttft: 100,
			requestTime: 500,
			tps: 10,
			cacheHitRate: 0.1,
		};
		renderPage();
		expect(screen.getAllByTestId("trend-chart")).toHaveLength(2);
		expect(screen.queryByTestId("pie-chart")).toBeNull();
		expect(screen.queryByTestId("rank-chart")).toBeNull();
	});

	it("渲染单模型指标卡片（6 个 StatsCard）", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.metrics = {
			providerName: "火山方舟",
			modelId: "deepseek-v3",
			totalTokens: 1_000_000,
			requestCount: 10,
			ttft: 120.5,
			requestTime: 800,
			tps: 55.25,
			cacheHitRate: 0.32,
		};
		renderPage();
		expect(screen.getByText("总计 Token")).toBeTruthy();
		expect(screen.getByText("100 万")).toBeTruthy();
		expect(screen.getByText("请求数")).toBeTruthy();
		expect(screen.getByText("TTFT")).toBeTruthy();
		expect(screen.getByText("120.5 ms")).toBeTruthy();
		expect(screen.getByText("TPS")).toBeTruthy();
		expect(screen.getByText("平均耗时")).toBeTruthy();
		expect(screen.getByText("缓存命中率")).toBeTruthy();
		expect(screen.getByText("32%")).toBeTruthy();
	});
});
