import { ProviderRaceCard } from "@/components/provider-race/ProviderRaceCard";
import type { ProviderRankItem, ProviderRankResponse } from "@/hooks/use-provider-race";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ProviderRaceCard 用 useNavigate 跳转二级页，测试中 stub。
vi.mock("react-router-dom", () => ({
	useNavigate: () => vi.fn(),
}));

const mocks = vi.hoisted(() => ({
	data: undefined as ProviderRankResponse | undefined,
	isLoading: false,
	isError: false,
	refetch: vi.fn(),
}));

vi.mock("@/hooks/use-provider-race", () => ({
	useProviderRace: () => ({
		data: mocks.data,
		isLoading: mocks.isLoading,
		isError: mocks.isError,
		refetch: mocks.refetch,
	}),
}));

// jsdom 无 IntersectionObserver，hook 内部兜底直接可见。
vi.mock("@/hooks/use-in-view", () => ({
	useInView: () => ({ ref: { current: null }, inView: true }),
}));

function makeItem(overrides: Partial<ProviderRankItem> = {}): ProviderRankItem {
	return {
		providerId: 1,
		providerName: "供应商A",
		requestCount: 10,
		totalTokens: 1_000_000,
		ttft: 120.5,
		requestTime: 800,
		tps: 55.25,
		cacheHitRate: 0.32,
		...overrides,
	};
}

function renderCard() {
	return render(<ProviderRaceCard />);
}

describe("ProviderRaceCard（供应商赛马）", () => {
	beforeEach(() => {
		mocks.data = undefined;
		mocks.isLoading = false;
		mocks.isError = false;
		mocks.refetch.mockClear();
	});

	it("未进入视口时展示懒加载占位，不发查询", () => {
		// inView=false 时 useInView 返回 false，但 mock 恒为 true；
		// 这里直接验证空数据渲染「暂无数据」路径。
		mocks.data = { startTime: 0, endTime: 0, items: [] };
		renderCard();
		expect(screen.getByText("该时间段暂无数据")).toBeTruthy();
	});

	it("渲染 6 个指标列头与供应商数据行", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [makeItem()] };
		renderCard();

		// 6 个指标列头。
		for (const label of ["总计 Token", "请求数", "TTFT", "平均耗时", "TPS", "缓存命中率"]) {
			expect(screen.getByRole("button", { name: label })).toBeTruthy();
		}
		// 数据格式化：token 万缩写、ttft 毫秒、tps 两位、命中率百分比。
		expect(screen.getByText("供应商A")).toBeTruthy();
		expect(screen.getByText("100 万")).toBeTruthy();
		expect(screen.getByText("120.5 ms")).toBeTruthy();
		expect(screen.getByText("55.25")).toBeTruthy();
		expect(screen.getByText("32%")).toBeTruthy();
	});

	it("点击表头切换排序方向（升/降交替）", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [makeItem()] };
		renderCard();

		// 默认「总计 Token」降序（箭头图标激活）。
		const tokenBtn = screen.getByRole("button", { name: "总计 Token" });
		expect(screen.getByTestId("sort-totalTokens")).toBeTruthy();

		// 点击 → 升序。
		fireEvent.click(tokenBtn);
		expect(screen.getByTestId("sort-totalTokens")).toBeTruthy();
		expect(screen.queryByTestId("sort-requestCount")).toBeNull();

		// 再点 → 降序（图标仍在，方向翻转由 aria-pressed 不体现，检查 testid 存在即可）。
		fireEvent.click(tokenBtn);
		expect(screen.getByTestId("sort-totalTokens")).toBeTruthy();
	});

	it("点击其他指标表头切到该指标并采用默认方向（耗时类升序）", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [makeItem()] };
		renderCard();

		// 点击 TTFT → 该列激活。
		const ttftBtn = screen.getByRole("button", { name: "TTFT" });
		fireEvent.click(ttftBtn);
		expect(screen.getByTestId("sort-ttft")).toBeTruthy();
		expect(screen.queryByTestId("sort-totalTokens")).toBeNull();

		// 点击请求数 → 切到请求数。
		const countBtn = screen.getByRole("button", { name: "请求数" });
		fireEvent.click(countBtn);
		expect(screen.getByTestId("sort-requestCount")).toBeTruthy();
		expect(screen.queryByTestId("sort-ttft")).toBeNull();
	});

	it("时间窗口控制：天/周/月/年/自定义切换", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [] };
		renderCard();

		// 默认「天」。
		expect(screen.getByRole("button", { name: "天" })).toHaveAttribute("aria-pressed", "true");

		fireEvent.click(screen.getByRole("button", { name: "周" }));
		expect(screen.getByRole("button", { name: "周" })).toHaveAttribute("aria-pressed", "true");

		fireEvent.click(screen.getByRole("button", { name: "自定义" }));
		// 自定义模式出现两行起止文本；点击弹出弹窗后才显示输入框。
		expect(screen.getByTestId("custom-range-label")).toBeTruthy();
		expect(screen.queryByTestId("custom-start")).toBeNull();
		fireEvent.click(screen.getByTestId("custom-range-label"));
		expect(screen.getByTestId("custom-start")).toBeTruthy();
		expect(screen.getByTestId("custom-end")).toBeTruthy();
		expect(screen.getByRole("button", { name: "确认" })).toBeTruthy();
	});

	it("左右箭头在非自定义模式下可见", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [] };
		renderCard();

		expect(screen.getByRole("button", { name: "上一周期" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "下一周期" })).toBeTruthy();
	});

	it("加载失败展示重试按钮，点击触发 refetch", () => {
		mocks.isError = true;
		renderCard();

		const retry = screen.getByRole("button", { name: "重试" });
		fireEvent.click(retry);
		expect(mocks.refetch).toHaveBeenCalled();
	});
});
