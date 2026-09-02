import { ProviderUsageCard, usageEnabled } from "@/components/providers/ProviderUsageCard";
import type { ProviderUsage } from "@/hooks/use-provider-usage";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	useProviderUsage: vi.fn(),
	refetch: vi.fn(),
}));

vi.mock("@/hooks/use-provider-usage", () => ({
	useProviderUsage: mocks.useProviderUsage,
}));

function mockQuery(result: Partial<ReturnType<typeof mocks.useProviderUsage>>) {
	mocks.useProviderUsage.mockReturnValue({
		data: undefined,
		isLoading: false,
		isFetching: false,
		error: null,
		refetch: mocks.refetch,
		...result,
	});
}

function quotaUsage(windows: ProviderUsage["windows"]): ProviderUsage {
	return {
		providerId: 1,
		fetchedAt: "2026-08-30T12:00:00Z",
		kind: "quota",
		plan: "pro",
		windows,
	};
}

describe("usageEnabled", () => {
	it("仅当 extra.usage === true 时开启", () => {
		expect(usageEnabled('{"usage": true, "usage_type": 1}')).toBe(true);
		expect(usageEnabled('{"usage": false}')).toBe(false);
		expect(usageEnabled("{}")).toBe(false);
		expect(usageEnabled("not json")).toBe(false);
	});
});

describe("ProviderUsageCard", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("加载中渲染骨架屏", () => {
		mockQuery({ isLoading: true });
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("用量信息")).toBeInTheDocument();
		expect(document.querySelectorAll("[class*='animate-pulse']").length).toBeGreaterThan(0);
	});

	it("quota：只渲染 available 的窗口（Kimi 式无月窗）", () => {
		mockQuery({
			data: quotaUsage([
				{
					window: "five_hour",
					available: true,
					usedPercent: 25,
					remainingPercent: 75,
					resetsAt: "2026-08-30T18:00:00Z",
				},
				{ window: "weekly", available: true, usedPercent: 10, remainingPercent: 90 },
				{ window: "monthly", available: false },
			]),
		});
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("5 小时")).toBeInTheDocument();
		expect(screen.getByText("本周")).toBeInTheDocument();
		expect(screen.queryByText("本月")).not.toBeInTheDocument();
		expect(screen.getByText("剩余 75%")).toBeInTheDocument();
		expect(screen.getByText("pro")).toBeInTheDocument();
		expect(screen.getByText(/重置/)).toBeInTheDocument();
	});

	it("quota：全部窗口不可用时显示空态", () => {
		mockQuery({
			data: quotaUsage([
				{ window: "five_hour", available: false },
				{ window: "weekly", available: false },
				{ window: "monthly", available: false },
			]),
		});
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("暂无用量数据")).toBeInTheDocument();
	});

	it("quota：无百分比时展示已用/总量绝对值", () => {
		mockQuery({
			data: quotaUsage([
				{ window: "five_hour", available: true, used: 57.2, limit: 800, unit: "flows" },
				{ window: "weekly", available: false },
				{ window: "monthly", available: false },
			]),
		});
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("已用 57.2 / 800 flows")).toBeInTheDocument();
	});

	it("balance：渲染余额条目与币种", () => {
		mockQuery({
			data: {
				providerId: 1,
				fetchedAt: "2026-08-30T12:00:00Z",
				kind: "balance",
				balances: [
					{ label: "余额（CNY）", amount: 110, currency: "CNY" },
					{ label: "充值余额（CNY）", amount: 100, currency: "CNY" },
				],
			} satisfies ProviderUsage,
		});
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("余额（CNY）")).toBeInTheDocument();
		expect(screen.getByText("110")).toBeInTheDocument();
		expect(screen.getAllByText("CNY").length).toBeGreaterThan(0);
	});

	it("错误态展示消息并支持重试（重试走 refresh=1 绕过缓存）", () => {
		mockQuery({ error: new Error("用量查询凭据无效或已过期") });
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("用量查询凭据无效或已过期")).toBeInTheDocument();
		fireEvent.click(screen.getByText("重试"));
		expect(mocks.useProviderUsage).toHaveBeenLastCalledWith(1, 1);
	});

	it("手动刷新递增 refreshToken 并带上 providerId", () => {
		mockQuery({});
		render(<ProviderUsageCard providerId={7} />);
		expect(mocks.useProviderUsage).toHaveBeenCalledWith(7, 0);
		fireEvent.click(screen.getByLabelText("刷新用量"));
		expect(mocks.useProviderUsage).toHaveBeenCalledWith(7, 1);
	});
});

describe("ProviderUsageCard 积分池徽标", () => {
	beforeEach(() => {
		vi.clearAllMocks();
	});

	it("窗口带 label 时显示池名徽标，多池同类窗口不撞 key", () => {
		mockQuery({
			data: quotaUsage([
				{
					window: "five_hour",
					available: true,
					remainingPercent: 55,
					label: "通用积分池",
				},
				{
					window: "five_hour",
					available: true,
					remainingPercent: 1,
					label: "Flash-Lite积分池",
				},
			]),
		});
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("通用积分池")).toBeInTheDocument();
		expect(screen.getByText("Flash-Lite积分池")).toBeInTheDocument();
		// 两个池各自渲染剩余百分比。
		expect(screen.getByText("剩余 55%")).toBeInTheDocument();
		expect(screen.getByText("剩余 1%")).toBeInTheDocument();
	});

	it("无 label 的窗口展示不变（不渲染徽标）", () => {
		mockQuery({
			data: quotaUsage([{ window: "weekly", available: true, remainingPercent: 90 }]),
		});
		render(<ProviderUsageCard providerId={1} />);
		expect(screen.getByText("本周")).toBeInTheDocument();
		expect(screen.queryByText("通用积分池")).not.toBeInTheDocument();
	});
});
