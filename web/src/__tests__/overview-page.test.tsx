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
	todaySummary: undefined as DashboardSummary | undefined,
	charts: undefined as DashboardCharts | undefined,
	summaryLoading: false,
	chartsLoading: false,
	summaryError: false,
	chartsError: false,
	refetch: vi.fn(),
	/** useDashboardSummary 每次调用参数（累计 + 今日各一次）。 */
	summaryParamsList: [] as Record<string, unknown>[],
	/** useDashboardCharts 每次调用参数（首页调用/token 两块各一次）。 */
	chartsParamsList: [] as Record<string, unknown>[],
}));

vi.mock("@/hooks/use-dashboard-stats", () => ({
	useDashboardSummary: (params?: unknown) => {
		mocks.summaryParamsList.push(params as Record<string, unknown>);
		// 今日（带时间窗口）与累计（不带）走不同分支返回。
		const p = params as Record<string, unknown> | undefined;
		const isToday = p?.startTime !== undefined || p?.endTime !== undefined;
		return {
			data: isToday ? mocks.todaySummary : mocks.summary,
			isLoading: mocks.summaryLoading,
			isError: mocks.summaryError,
			refetch: mocks.refetch,
		};
	},
	useDashboardCharts: (params?: unknown) => {
		mocks.chartsParamsList.push(params as Record<string, unknown>);
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
			callTrend: [],
			failureTrend: [],
			failureRateTrend: [],
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

vi.mock("@/hooks/use-api-key-race", () => ({
	useApiKeyRace: () => ({
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
		mocks.todaySummary = undefined;
		mocks.charts = undefined;
		mocks.summaryLoading = false;
		mocks.chartsLoading = false;
		mocks.summaryError = false;
		mocks.chartsError = false;
		mocks.refetch.mockClear();
		mocks.summaryParamsList = [];
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
		mocks.todaySummary = makeSummary({
			totalRequests: 1,
			successRate: 0,
			totalTokens: 1,
			cacheHitRate: 0,
		});
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		// 累计行（今日行数值已被差异化，不会撞文本）。
		expect(screen.getByText("累计请求数")).toBeTruthy();
		expect(screen.getByText("12,345")).toBeTruthy();
		expect(screen.getByText("75.6%")).toBeTruthy();
		expect(screen.getByText("1.23 亿")).toBeTruthy();
		expect(screen.getByText("30.4%")).toBeTruthy();
	});

	it("累计指标下方再渲染四个今日指标（副标题今日）", () => {
		mocks.summary = makeSummary();
		mocks.todaySummary = makeSummary({
			totalRequests: 321,
			successRate: 0.9,
			totalTokens: 12_345,
			cacheHitRate: 0.1,
		});
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		// 今日请求数标签与今日副标题（今日行四张卡副标题均为「今日」）。
		expect(screen.getByText("今日请求数")).toBeTruthy();
		expect(screen.getAllByText("今日").length).toBe(4);
		// 今日行的四个值（累计行已用默认值 12,345/75.6%/1.23 亿/30.4%，不会撞今日行文本）。
		expect(screen.getByText("321")).toBeTruthy();
		expect(screen.getByText("90%")).toBeTruthy();
		expect(screen.getByText("1.2 万")).toBeTruthy();
		expect(screen.getByText("10%")).toBeTruthy();
	});

	it("今日 summary 请求携带本地今日窗口，累计请求不带时间参数", () => {
		mocks.summary = makeSummary();
		mocks.todaySummary = makeSummary();
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		const calls = mocks.summaryParamsList;
		expect(calls.length).toBe(2);
		const todayCall = calls.find((p) => p?.startTime !== undefined);
		const allCall = calls.find((p) => p?.startTime === undefined);
		expect(allCall?.endTime).toBeUndefined();
		// 今日窗口：本地今日 0 点 → 当前时刻（endTime 在 render 时捕获，容差放宽防 CI 抖动）。
		const now = Date.now();
		const startOfToday = new Date(now);
		startOfToday.setHours(0, 0, 0, 0);
		expect(todayCall?.startTime).toBe(startOfToday.getTime());
		expect(Number(todayCall?.endTime)).toBeCloseTo(now, -3);
	});

	it("默认展示两个折线图（调用趋势 + token 使用分布）", () => {
		mocks.summary = makeSummary();
		mocks.todaySummary = makeSummary();
		mocks.charts = makeCharts();
		render(<OverviewPage />);

		expect(screen.getAllByTestId("trend-chart")).toHaveLength(2);
		expect(screen.queryByTestId("pie-chart")).toBeNull();
	});

	it("调用分析三态切换：分布 → 饼图，排行 → 条形图", () => {
		mocks.summary = makeSummary();
		mocks.todaySummary = makeSummary();
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
		mocks.todaySummary = makeSummary();
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
		mocks.todaySummary = makeSummary();
		mocks.charts = makeCharts({ callByModel: [] });
		render(<OverviewPage />);

		fireEvent.click(screen.getByRole("button", { name: "调用次数分布" }));
		expect(screen.getByText("暂无调用数据")).toBeTruthy();
	});
});

describe("OverviewPage 时间组件（默认今天，调用/Token/可靠性三块独立）", () => {
	const withinWindow = (testId: string) => {
		const container = screen.getByTestId(testId);
		return within(container);
	};

	beforeEach(() => {
		mocks.summary = makeSummary();
		mocks.todaySummary = makeSummary();
		mocks.charts = makeCharts();
		mocks.summaryParamsList = [];
		mocks.chartsParamsList = [];
	});

	it("默认选中「天」，调用与 Token 图表请求均携带小时粒度与本地时区偏移", () => {
		render(<OverviewPage />);

		// 三块控件默认「天」处于按下态。
		for (const testId of ["call-window", "token-window", "insight-window"]) {
			expect(withinWindow(testId).getByRole("button", { name: "天" })).toHaveAttribute(
				"aria-pressed",
				"true",
			);
		}
		// 调用与 Token 各发一次 charts 请求，均携带显式窗口 + granularity=hour + tzOffsetMinutes。
		expect(mocks.chartsParamsList.length).toBe(2);
		for (const params of mocks.chartsParamsList) {
			expect(params?.granularity).toBe("hour");
			expect(typeof params?.tzOffsetMinutes).toBe("number");
			expect(params?.startTime).toBeTypeOf("number");
			expect(params?.endTime).toBeTypeOf("number");
		}
	});

	it("调用分析切换到「周」后，仅调用请求携带 day 粒度，Token 仍为 hour", () => {
		render(<OverviewPage />);

		fireEvent.click(withinWindow("call-window").getByRole("button", { name: "周" }));
		expect(withinWindow("call-window").getByRole("button", { name: "周" })).toHaveAttribute(
			"aria-pressed",
			"true",
		);
		// 最近一次渲染的两次调用：一次 day（调用窗口）、一次 hour（token 窗口），顺序无关。
		const recent = mocks.chartsParamsList
			.slice(-2)
			.map((p) => p?.granularity)
			.sort();
		expect(recent).toEqual(["day", "hour"]);
	});

	it("切换到「年」后请求携带 month 粒度，且副标题显示当前年份", () => {
		render(<OverviewPage />);

		fireEvent.click(withinWindow("call-window").getByRole("button", { name: "年" }));
		const recent = mocks.chartsParamsList
			.slice(-2)
			.map((p) => p?.granularity)
			.sort();
		expect(recent).toEqual(["hour", "month"]);
		// 副标题与控件标题都显示当前年份（同文本出现两处）。
		expect(
			withinWindow("call-window").getAllByText(`${new Date().getFullYear()}年（当前）`).length,
		).toBeGreaterThan(0);
	});

	it("切换到「自定义」显示两行起止文本，点击弹出弹窗选时间", () => {
		render(<OverviewPage />);

		fireEvent.click(withinWindow("call-window").getByRole("button", { name: "自定义" }));
		// 两行文本展示开始/结束（默认过去 7 天）。
		expect(withinWindow("call-window").getByTestId("custom-range-label")).toBeTruthy();
		expect(withinWindow("call-window").getByText(/^开始 /)).toBeTruthy();
		expect(withinWindow("call-window").getByText(/^结束 /)).toBeTruthy();
		// 默认窗口：开始 = 7 天前 0 点，结束 = 明天 0 点（显示为今天 24:00:00）。
		// 调用窗口为自定义（7 天 → day 粒度），token 窗口仍默认「天」（hour 粒度）。
		const recent = mocks.chartsParamsList
			.slice(-2)
			.map((p) => p?.granularity)
			.sort();
		expect(recent).toEqual(["day", "hour"]);

		// 点击两行文本 → 弹出弹窗，内含起止输入框与确认按钮。
		fireEvent.click(withinWindow("call-window").getByTestId("custom-range-label"));
		expect(screen.getByTestId("custom-start")).toBeTruthy();
		expect(screen.getByTestId("custom-end")).toBeTruthy();
		expect(screen.getByRole("button", { name: "确认" })).toBeTruthy();
	});
});
