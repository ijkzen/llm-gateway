import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import VirtualModelOverviewPage from "@/pages/virtual-model-overview";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	callCharts: undefined as DashboardCharts | undefined,
	tokenCharts: undefined as DashboardCharts | undefined,
	callLoading: false,
	tokenLoading: false,
	displayId: "deepseek-v4-flash",
	memberItems: [] as Array<Record<string, unknown>>,
	memberLoading: false,
	memberError: false,
}));

vi.mock("react-router-dom", () => ({
	useParams: () => ({ virtualModelId: "3" }),
	useSearchParams: () => [new URLSearchParams("period=week&offset=0"), vi.fn()],
	useNavigate: () => vi.fn(),
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

vi.mock("@/hooks/use-virtual-models", () => ({
	useVirtualModelDetail: () => ({ data: { displayId: mocks.displayId } }),
}));

vi.mock("@/hooks/use-virtual-model-member-rank", () => ({
	useVirtualModelMemberRank: () => ({
		data: { startTime: 0, endTime: 0, items: mocks.memberItems },
		isLoading: mocks.memberLoading,
		isError: mocks.memberError,
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

vi.mock("@/hooks/use-in-view", () => ({
	useInView: () => ({ ref: { current: null }, inView: true }),
}));

vi.mock("@/hooks/use-stats-metrics", () => ({
	useVirtualModelMetrics: () => ({
		data: {
			virtualModelId: 3,
			virtualModelDisplayId: mocks.displayId,
			requestCount: 10,
			totalTokens: 2000,
			ttft: 100,
			requestTime: 500,
			tps: 20,
			cacheHitRate: 0.3,
		},
		isLoading: false,
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
	return render(<VirtualModelOverviewPage />);
}

describe("VirtualModelOverviewPage（虚拟模型二级数据面板）", () => {
	beforeEach(() => {
		mocks.callCharts = undefined;
		mocks.tokenCharts = undefined;
		mocks.callLoading = false;
		mocks.tokenLoading = false;
		mocks.memberItems = [];
		mocks.memberLoading = false;
		mocks.memberError = false;
	});

	it("页头显示虚拟模型名 + 数据面板", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getByText("deepseek-v4-flash · 数据面板")).toBeTruthy();
	});

	it("URL 带时间段参数时三块初始化为该时间段（周）", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(3);
	});

	it("三个区块各自独立：改调用分析的时间不影响 token/赛马", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();

		const monthButtons = screen.getAllByRole("button", { name: "月" });
		const firstMonth = monthButtons[0];
		if (!firstMonth) {
			throw new Error("找不到「月」按钮");
		}
		fireEvent.click(firstMonth);
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(2);
	});

	it("渲染调用分析 + token 分析图表卡片", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getAllByTestId("trend-chart").length).toBeGreaterThanOrEqual(2);
	});

	it("成员模型赛马渲染成员行（含停用标记）", () => {
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		mocks.memberItems = [
			{
				providerId: 1,
				providerName: "火山方舟",
				modelId: "deepseek-v3",
				memberEnable: true,
				requestCount: 10,
				totalTokens: 1_000_000,
				ttft: 120.5,
				requestTime: 800,
				tps: 55.25,
				cacheHitRate: 0.32,
			},
			{
				providerId: 2,
				providerName: "DeepSeek 官方",
				modelId: "deepseek-chat",
				memberEnable: false,
				requestCount: 0,
				totalTokens: 0,
				ttft: 0,
				requestTime: 0,
				tps: 0,
				cacheHitRate: 0,
			},
		];
		renderPage();
		expect(screen.getByText("火山方舟・deepseek-v3")).toBeTruthy();
		// 停用标记是独立 span，分开断言。
		expect(screen.getByText("DeepSeek 官方・deepseek-chat")).toBeTruthy();
		expect(screen.getByText("（停用）")).toBeTruthy();
		expect(screen.getByText("100 万")).toBeTruthy();
	});
});
