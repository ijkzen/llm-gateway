import { InsightAnalysisCard } from "@/components/insight-analysis-card";
import type { InsightData } from "@/hooks/use-dashboard-insight";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// 捕获 FailureTrendChart 收到的 props，验证 callTrend 是否为真实调用趋势（而非失败趋势）。
const trendProps = vi.hoisted(() => ({
	failureTrendChart: { callTrend: [], failureTrend: [], failureRateTrend: [] } as {
		callTrend: Array<{ bucketStart: number; value: number }>;
		failureTrend: Array<{ bucketStart: number; value: number }>;
		failureRateTrend: Array<{ bucketStart: number; value: number }>;
	},
}));

vi.mock("@/components/insight-charts", () => ({
	FailureTrendChart: (props: typeof trendProps.failureTrendChart) => {
		trendProps.failureTrendChart = props;
		return <div data-testid="failure-trend" />;
	},
	PercentileLineChart: () => <div data-testid="percentile-chart" />,
	TokenStructureChart: () => <div data-testid="token-structure" />,
	OutputPerSecLineChart: () => <div data-testid="output-per-sec" />,
	ThroughputChart: () => <div data-testid="throughput" />,
}));

function makeInsight(overrides: Partial<InsightData> = {}): InsightData {
	return {
		callTrend: [{ bucketStart: 1_700_000_000_000, value: 10 }],
		failureTrend: [{ bucketStart: 1_700_000_000_000, value: 2 }],
		failureRateTrend: [{ bucketStart: 1_700_000_000_000, value: 0.2 }],
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

describe("InsightAnalysisCard 失败诊断（FailureTrendChart props 传递）", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("FailureTrendChart 的 callTrend 应为调用趋势而非失败趋势（回归：success 恒 0 bug）", () => {
		// 场景：窗口有调用（apiKeyRank 非空）+ 失败趋势有值。
		// 若 callTrend 被错误地传成 failureTrend，success=0 恒成立。
		render(<InsightAnalysisCard data={makeInsight()} />);

		expect(screen.getByTestId("failure-trend")).toBeInTheDocument();
		const { callTrend, failureTrend } = trendProps.failureTrendChart;
		expect(failureTrend.length).toBeGreaterThan(0);
		// callTrend 应不等于 failureTrend（失败 2 次 ≠ 调用 10 次）。
		expect(callTrend).not.toEqual(failureTrend);
		// callTrend 的值应大于等于失败趋势（调用数 ≥ 失败数）。
		for (let i = 0; i < failureTrend.length; i++) {
			expect(callTrend[i]?.value ?? 0).toBeGreaterThanOrEqual(failureTrend[i]?.value ?? 0);
		}
	});

	it("失败诊断有流量时渲染失败趋势折线图（无失败原因条形图）", () => {
		render(<InsightAnalysisCard data={makeInsight()} />);
		expect(screen.getByTestId("failure-trend")).toBeInTheDocument();
		// 失败原因条形图已按用户决策移除。
		expect(screen.queryByTestId("failure-reasons")).toBeNull();
	});
});
