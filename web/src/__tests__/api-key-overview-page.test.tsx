import type { ApiKeyDetail } from "@/hooks/use-api-keys";
import type { DashboardCharts } from "@/hooks/use-dashboard-stats";
import type { ApiKeyMetrics } from "@/hooks/use-stats-metrics";
import ApiKeyOverviewPage from "@/pages/api-key-overview";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	detail: undefined as ApiKeyDetail | undefined,
	detailError: false,
	metrics: undefined as ApiKeyMetrics | undefined,
	metricsLoading: false,
	callCharts: undefined as DashboardCharts | undefined,
	tokenCharts: undefined as DashboardCharts | undefined,
	navigate: vi.fn(),
}));

vi.mock("react-router-dom", async () => {
	const actual = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
	return {
		...actual,
		useParams: () => ({ id: "3" }),
		useSearchParams: () => [new URLSearchParams("period=week&offset=0"), vi.fn()],
		useNavigate: () => mocks.navigate,
	};
});

vi.mock("@/hooks/use-api-keys", () => ({
	useApiKeyDetail: () => ({
		data: mocks.detail,
		isLoading: false,
		isError: mocks.detailError,
		refetch: vi.fn(),
	}),
}));

vi.mock("@/hooks/use-stats-metrics", () => ({
	useApiKeyMetrics: () => ({
		data: mocks.metrics,
		isLoading: mocks.metricsLoading,
		isError: false,
		refetch: vi.fn(),
	}),
}));

vi.mock("@/hooks/use-dashboard-stats", () => ({
	useDashboardCharts: () => ({
		data: mocks.callCharts,
		isLoading: false,
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

// 三张排行卡内部 hook：mock 返回空数据（具体行断言在需要时补充）。
const emptyRank = { startTime: 0, endTime: 0, items: [] };
vi.mock("@/hooks/use-provider-race", () => ({
	useProviderRace: () => ({
		data: emptyRank,
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));
vi.mock("@/hooks/use-virtual-model-race", () => ({
	useVirtualModelRace: () => ({
		data: emptyRank,
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));
vi.mock("@/hooks/use-provider-model-race", () => ({
	useProviderModelRace: () => ({
		data: emptyRank,
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
	}),
}));
vi.mock("@/hooks/use-in-view", () => ({
	useInView: () => ({ ref: { current: null }, inView: true }),
}));

// 折线图依赖浏览器布局，stub 标记视图。
vi.mock("@/components/dashboard-charts", () => ({
	TrendLineChart: () => <div data-testid="trend-chart" />,
	ModelPieChart: () => <div data-testid="pie-chart" />,
	ModelRankBarChart: () => <div data-testid="rank-chart" />,
}));

function makeDetail(overrides: Partial<ApiKeyDetail> = {}): ApiKeyDetail {
	return {
		id: 3,
		name: "prod-key",
		keyMasked: "lg-****abcd",
		key: "lg-plain",
		enable: true,
		createdAt: "2026-08-29T00:00:00Z",
		updatedAt: "2026-08-29T00:00:00Z",
		...overrides,
	};
}

function makeMetrics(overrides: Partial<ApiKeyMetrics> = {}): ApiKeyMetrics {
	return {
		apiKeyName: "prod-key",
		totalTokens: 1_000_000,
		requestCount: 10,
		ttft: 120.5,
		requestTime: 800,
		tps: 55.25,
		cacheHitRate: 0.32,
		...overrides,
	};
}

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
	return render(<ApiKeyOverviewPage />);
}

describe("ApiKeyOverviewPage（API Key 数据面板）", () => {
	beforeEach(() => {
		mocks.detail = undefined;
		mocks.detailError = false;
		mocks.metrics = undefined;
		mocks.metricsLoading = false;
		mocks.callCharts = undefined;
		mocks.tokenCharts = undefined;
		mocks.navigate.mockClear();
	});

	it("key 已删除 / 详情失败：显示错误态并引导返回列表", () => {
		mocks.detailError = true;
		renderPage();
		expect(screen.getByText("API Key 数据面板不可用")).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: "返回 API Key 列表" }));
		expect(mocks.navigate).toHaveBeenCalledWith("/api-keys");
	});

	it("页头显示 key 名 + 数据面板后缀，渲染顶部 6 指标卡", () => {
		mocks.detail = makeDetail();
		mocks.metrics = makeMetrics();
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getByText("prod-key · 数据面板")).toBeTruthy();
		expect(screen.getByText("总计 Token")).toBeTruthy();
		expect(screen.getByText("100 万")).toBeTruthy();
		expect(screen.getByText("TTFT")).toBeTruthy();
		expect(screen.getByText("120.5 ms")).toBeTruthy();
	});

	it("URL 带时间段参数时区块初始化为该时间段（周）", () => {
		mocks.detail = makeDetail();
		mocks.metrics = makeMetrics();
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(3);
	});

	it("渲染调用分析与 Token 分析折线区块", () => {
		mocks.detail = makeDetail();
		mocks.metrics = makeMetrics();
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		// 调用/Token 卡默认 trend 视图各渲染一个折线图。
		expect(screen.getAllByTestId("trend-chart").length).toBeGreaterThanOrEqual(2);
	});

	it("渲染三张排行卡（该 key 用到的虚拟模型 / 供应商 / 模型）", () => {
		mocks.detail = makeDetail();
		mocks.metrics = makeMetrics();
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();
		expect(screen.getByText("虚拟模型赛马")).toBeTruthy();
		expect(screen.getByText("供应商赛马")).toBeTruthy();
		expect(screen.getByText("供应商模型赛马")).toBeTruthy();
	});

	it("区块独立时间窗：切换顶部指标卡时间段不影响其它区块周选择", () => {
		mocks.detail = makeDetail();
		mocks.metrics = makeMetrics();
		mocks.callCharts = makeCharts();
		mocks.tokenCharts = makeCharts();
		renderPage();

		// 顶部指标卡切到「月」。
		const monthButtons = screen.getAllByRole("button", { name: "月" });
		if (!monthButtons[0]) {
			throw new Error("找不到「月」按钮");
		}
		fireEvent.click(monthButtons[0]);
		// 其它区块仍保留「周」（URL 初始窗）。
		expect(screen.getAllByRole("button", { name: "周" }).length).toBeGreaterThanOrEqual(3);
	});
});
