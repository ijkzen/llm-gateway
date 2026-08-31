import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import type { InsightData } from "@/hooks/use-dashboard-insight";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// recharts 在 jsdom 下依赖布局测量，测试中 stub 各视图标记。
vi.mock("@/components/insight-charts", () => ({
	FailureTrendChart: () => <div data-testid="failure-trend" />,
	FailureReasonBarChart: () => <div data-testid="failure-reasons" />,
	PercentileLineChart: () => <div data-testid="percentile-chart" />,
	TokenStructureChart: () => <div data-testid="token-structure" />,
	OutputPerSecLineChart: () => <div data-testid="output-per-sec" />,
	ThroughputChart: () => <div data-testid="throughput" />,
}));

function makeInsight(overrides: Partial<InsightData> = {}): InsightData {
	return {
		failureTrend: [{ bucketStart: 1_700_000_000_000, value: 2 }],
		failureRateTrend: [{ bucketStart: 1_700_000_000_000, value: 0.2 }],
		failureReasons: [{ reason: "上游 429", count: 1 }],
		ttftPercentiles: [{ bucketStart: 1_700_000_000_000, p50: 100, p90: 300, p95: 400, p99: 600 }],
		latencyPercentiles: [
			{ bucketStart: 1_700_000_000_000, p50: 500, p90: 900, p95: 1200, p99: 2000 },
		],
		inputTokenTrend: [{ bucketStart: 1_700_000_000_000, value: 1000 }],
		outputTokenTrend: [{ bucketStart: 1_700_000_000_000, value: 2000 }],
		cacheHitRateTrend: [{ bucketStart: 1_700_000_000_000, value: 0.3 }],
		outputTokensPerSecTrend: [{ bucketStart: 1_700_000_000_000, value: 50 }],
		apiKeyRank: [{ apiKeyName: "key-1", value: 10 }],
		rpmTrend: [{ bucketStart: 1_700_000_000_000, value: 5 }],
		tpmTrend: [{ bucketStart: 1_700_000_000_000, value: 500 }],
		streamRatioTrend: [{ bucketStart: 1_700_000_000_000, value: 0.8 }],
		...overrides,
	};
}

describe("InsightAnalysisCard（性能与可靠性分析）", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("默认渲染失败诊断 Tab（失败趋势 + 失败原因）", () => {
		render(<InsightAnalysisCard data={makeInsight()} />);
		expect(screen.getByTestId("failure-trend")).toBeInTheDocument();
		expect(screen.getByTestId("failure-reasons")).toBeInTheDocument();
	});

	it("切到延迟分位 Tab 渲染 TTFT 与总耗时两组分位图", () => {
		render(<InsightAnalysisCard data={makeInsight()} />);
		fireEvent.click(screen.getByRole("button", { name: "延迟分位" }));
		// TTFT 与总耗时各一张分位图。
		expect(screen.getAllByTestId("percentile-chart")).toHaveLength(2);
	});

	it("切到 Token 结构 Tab 渲染堆叠图与每秒输出 token", () => {
		render(<InsightAnalysisCard data={makeInsight()} />);
		fireEvent.click(screen.getByRole("button", { name: "Token 结构" }));
		expect(screen.getByTestId("token-structure")).toBeInTheDocument();
		expect(screen.getByTestId("output-per-sec")).toBeInTheDocument();
	});

	it("切到吞吐 Tab 渲染 RPM/TPM 图", () => {
		render(<InsightAnalysisCard data={makeInsight()} />);
		fireEvent.click(screen.getByRole("button", { name: "吞吐" }));
		expect(screen.getByTestId("throughput")).toBeInTheDocument();
	});

	it("失败诊断空数据（窗口内无请求）时显示空态文案", () => {
		const data = makeInsight({
			apiKeyRank: [],
			failureTrend: [],
			failureRateTrend: [],
			failureReasons: [],
		});
		render(<InsightAnalysisCard data={data} />);
		expect(screen.getByText("该时间段暂无可靠性数据")).toBeInTheDocument();
	});

	it("有流量但零失败的健康日仍渲染失败图（不误判为空态）", () => {
		// 窗口有请求（apiKeyRank 非空）但失败趋势全 0：应展示失败图而非空态。
		const data = makeInsight({
			failureTrend: [{ bucketStart: 1_700_000_000_000, value: 0 }],
			failureRateTrend: [{ bucketStart: 1_700_000_000_000, value: 0 }],
			failureReasons: [],
		});
		render(<InsightAnalysisCard data={data} />);
		expect(screen.getByTestId("failure-trend")).toBeInTheDocument();
		expect(screen.queryByText("该时间段暂无可靠性数据")).not.toBeInTheDocument();
	});

	it("吞吐非小时桶（rpmTrend 为空）显示提示文案", () => {
		const data = makeInsight({ rpmTrend: [], tpmTrend: [], streamRatioTrend: [] });
		render(<InsightAnalysisCard data={data} />);
		fireEvent.click(screen.getByRole("button", { name: "吞吐" }));
		expect(screen.getByText(/吞吐（RPM\/TPM）仅在小时粒度可用/)).toBeInTheDocument();
	});
});
