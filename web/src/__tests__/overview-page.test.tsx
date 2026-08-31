import type { DashboardCharts, DashboardSummary } from "@/hooks/use-dashboard-stats";
import OverviewPage from "@/pages/overview";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ProviderRaceCard 用 useNavigate 跳转二级页，测试中 stub。
vi.mock("react-router-dom", () => ({
	useNavigate: () => vi.fn(),
}));

const mocks = vi.hoisted(() => ({
	summary: undefined as DashboardSummary | undefined,
	charts: undefined as DashboardCharts | undefined,
	summaryLoading: false,
	chartsLoading: false,
	summaryError: false,
	chartsError: false,
	refetch: vi.fn(),
	chartsParams: undefined as Record<string, unknown> | undefined,
}));

vi.mock("@/hooks/use-dashboard-stats", () => ({
	useDashboardSummary: () => ({
		data: mocks.summary,
		isLoading: mocks.summaryLoading,
		isError: mocks.summaryError,
		refetch: mocks.refetch,
	}),
	useDashboardCharts: (params?: unknown) => {
		mocks.chartsParams = params as Record<string, unknown> | undefined;
		return {
			data: mocks.charts,
			isLoading: mocks.chartsLoading,
			isError: mocks.chartsError,
			refetch: mocks.refetch,
		};
	},
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

// 图表组件依赖浏览器布局尺寸，jsdom 下用 stub 标记当前视图。
vi.mock("@/components/dashboard-charts", () => ({
	TrendLineChart: () => <div data-testid="trend-chart" />,
	ModelPieChart: () => <div data-testid="pie-chart" />,
	ModelRankBarChart: () => <div data-testid="rank-chart" />,
}));

// 赛马卡片使用 TanStack Query + IntersectionObserver，测试中直接 stub。
vi.mock("@/hooks/use-in-view", () => ({
	useInView: () => ({ ref: { current: null }, inView: true }),
}));

vi.mock("@/hooks/use-provider-race", () => ({
	useProviderRace: () => ({
		data: { startTime: 0, endTime: 0, items: [] },
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));

vi.mock("@/hooks/use-virtual-model-race", () => ({
	useVirtualModelRace: () => ({
		data: { startTime: 0, endTime: 0, items: [] },
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));

vi.mock("@/hooks/use-provider-model-race", () => ({
	useProviderModelRace: () => ({
		data: { startTime: 0, endTime: 0, items: [] },
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));

function makeSummary(overrides: Partial<DashboardSummary> = {}): DashboardSummary {
	return {
		totalRequests: 12345,
		successRate: 0.756,
		totalTokens: 123_456_789,
		cacheHitRate: 0.304,
		...overrides,
	};
}

function makeCharts(overrides: Partial<DashboardCharts> = {}): DashboardCharts {
	return {
		callTrend: [{ bucketStart: 1_700_000_000_000, value: 5 }],
		callByModel: [{ providerName: "", modelId: "gpt-4o", value: 5 }],
		tokenTrend: [{ bucketStart: 1_700_000_000_000, value: 500 }],
		tokenByModel: [{ providerName: "", modelId: "gpt-4o", value: 500 }],
		...overrides,
	};
}

describe("OverviewPage（数据面板）", () => {
	beforeEach(() => {
		mocks.summary = undefined;
		mocks.charts = undefined;
		mocks.summaryLoading = false;
		mocks.chartsLoading = false;
		mocks.summaryError = false;
		mocks.chartsError = false;
		mocks.refetch.mockClear();
	});

	it("加载中渲染骨架屏，不渲染图表卡片", () => {
		mocks.summaryLoading = true;
		render(<OverviewPage />);

		expect(screen.queryByText("调用分析")).toBeNull();
		expect(screen.queryByText("累计请求数")).toBeNull();
	});

	it("加载失败展示错误态", () => {
		mocks.summaryError = true;
		render(<OverviewPage />);

		expect(screen.getByText(/无法获取数据面板数据/)).toBeTruthy();
	});

	it("顶部渲染四个累计指标（token 按亿缩写、比率按百分比）", () => {
		mocks.summary = makeSummary();
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		expect(screen.getByText("累计请求数")).toBeTruthy();
		expect(screen.getByText("12,345")).toBeTruthy();
		expect(screen.getByText("75.6%")).toBeTruthy();
		expect(screen.getByText("1.23 亿")).toBeTruthy();
		expect(screen.getByText("30.4%")).toBeTruthy();
	});

	it("默认展示两个折线图（调用趋势 + token 使用分布）", () => {
		mocks.summary = makeSummary();
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		expect(screen.getAllByTestId("trend-chart")).toHaveLength(2);
		expect(screen.queryByTestId("pie-chart")).toBeNull();
	});

	it("调用分析三态切换：分布 → 饼图，排行 → 条形图", () => {
		mocks.summary = makeSummary();
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		fireEvent.click(screen.getByRole("button", { name: "调用次数分布" }));
		expect(screen.getByTestId("pie-chart")).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "调用次数排行" }));
		expect(screen.getByTestId("rank-chart")).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "调用趋势" }));
		expect(screen.getAllByTestId("trend-chart")).toHaveLength(2);
	});

	it("token 分析三态切换互不影响调用分析", () => {
		mocks.summary = makeSummary();
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		fireEvent.click(screen.getByRole("button", { name: "token 模型分布" }));
		expect(screen.getByTestId("pie-chart")).toBeTruthy();
		// 调用分析仍停留在折线图。
		expect(screen.getByTestId("trend-chart")).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "token 模型排行" }));
		expect(screen.getByTestId("rank-chart")).toBeTruthy();
	});

	it("模型维度无数据时展示空态", () => {
		mocks.summary = makeSummary();
		mocks.charts = makeCharts({ callByModel: [] });
		render(<OverviewPage />);

		fireEvent.click(screen.getByRole("button", { name: "调用次数分布" }));
		expect(screen.getByText("暂无调用数据")).toBeTruthy();
	});
});

describe("OverviewPage 时间组件（默认今天）", () => {
	const withinChartsWindow = () => {
		const container = screen.getByTestId("charts-window");
		return within(container);
	};

	beforeEach(() => {
		mocks.summary = makeSummary();
		mocks.charts = makeCharts();
		mocks.chartsParams = undefined;
	});

	it("默认选中「天」，图表请求携带小时粒度与本地时区偏移", () => {
		render(<OverviewPage />);

		// 默认「天」处于按下态。
		expect(withinChartsWindow().getByRole("button", { name: "天" })).toHaveAttribute(
			"aria-pressed",
			"true",
		);
		// 请求携带显式窗口 + granularity=hour + tzOffsetMinutes。
		expect(mocks.chartsParams).toBeTruthy();
		expect(mocks.chartsParams?.granularity).toBe("hour");
		expect(typeof mocks.chartsParams?.tzOffsetMinutes).toBe("number");
		expect(mocks.chartsParams?.startTime).toBeTypeOf("number");
		expect(mocks.chartsParams?.endTime).toBeTypeOf("number");
	});

	it("切换到「周」后请求携带 day 粒度", () => {
		render(<OverviewPage />);

		fireEvent.click(withinChartsWindow().getByRole("button", { name: "周" }));
		expect(withinChartsWindow().getByRole("button", { name: "周" })).toHaveAttribute(
			"aria-pressed",
			"true",
		);
		expect(mocks.chartsParams?.granularity).toBe("day");
	});

	it("切换到「年」后请求携带 month 粒度，且副标题显示当前年份", () => {
		render(<OverviewPage />);

		fireEvent.click(withinChartsWindow().getByRole("button", { name: "年" }));
		expect(mocks.chartsParams?.granularity).toBe("month");
		// 副标题与控件标题都显示当前年份（同文本出现两处）。
		expect(
			withinChartsWindow().getAllByText(`${new Date().getFullYear()}年（当前）`).length,
		).toBeGreaterThan(0);
	});

	it("切换到「自定义」显示两行起止文本，点击弹出弹窗选时间", () => {
		render(<OverviewPage />);

		fireEvent.click(withinChartsWindow().getByRole("button", { name: "自定义" }));
		// 两行文本展示开始/结束（默认过去 7 天）。
		expect(withinChartsWindow().getByTestId("custom-range-label")).toBeTruthy();
		expect(withinChartsWindow().getByText(/^开始 /)).toBeTruthy();
		expect(withinChartsWindow().getByText(/^结束 /)).toBeTruthy();
		// 默认窗口：开始 = 7 天前 0 点，结束 = 明天 0 点（显示为今天 24:00:00）。
		expect(mocks.chartsParams?.granularity).toBe("day");

		// 点击两行文本 → 弹出弹窗，内含起止输入框与确认按钮。
		fireEvent.click(withinChartsWindow().getByTestId("custom-range-label"));
		expect(screen.getByTestId("custom-start")).toBeTruthy();
		expect(screen.getByTestId("custom-end")).toBeTruthy();
		expect(screen.getByRole("button", { name: "确认" })).toBeTruthy();
	});
});
