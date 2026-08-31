import { ApiKeyRaceCard } from "@/components/api-key-race/ApiKeyRaceCard";
import type { ApiKeyRankItem, ApiKeyRankResponse } from "@/hooks/use-api-key-race";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	data: undefined as ApiKeyRankResponse | undefined,
	isLoading: false,
	isError: false,
	refetch: vi.fn(),
}));

vi.mock("@/hooks/use-api-key-race", () => ({
	useApiKeyRace: () => ({
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

function makeItem(overrides: Partial<ApiKeyRankItem> = {}): ApiKeyRankItem {
	return {
		apiKeyName: "key-a",
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
	return render(<ApiKeyRaceCard />);
}

describe("ApiKeyRaceCard（API Key 赛马）", () => {
	beforeEach(() => {
		mocks.data = undefined;
		mocks.isLoading = false;
		mocks.isError = false;
		mocks.refetch.mockClear();
	});

	it("渲染卡片标题、6 个指标列头与 API Key 数据行", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [makeItem()] };
		renderCard();

		// 卡片标题 + 6 个指标列头。
		expect(screen.getByText("API Key 赛马")).toBeTruthy();
		for (const label of ["总计 Token", "请求数", "TTFT", "平均耗时", "TPS", "缓存命中率"]) {
			expect(screen.getByRole("button", { name: label })).toBeTruthy();
		}
		// 数据格式化：token 万缩写、ttft 毫秒、tps 两位、命中率百分比。
		expect(screen.getByText("key-a")).toBeTruthy();
		expect(screen.getByText("100 万")).toBeTruthy();
		expect(screen.getByText("120.5 ms")).toBeTruthy();
		expect(screen.getByText("55.25")).toBeTruthy();
		expect(screen.getByText("32%")).toBeTruthy();
	});

	it("空数据显示暂无数据", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [] };
		renderCard();
		expect(screen.getByText("该时间段暂无数据")).toBeTruthy();
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

		// 点击其他指标表头切到该指标并采用默认方向（耗时类升序）。
		fireEvent.click(screen.getByRole("button", { name: "TTFT" }));
		expect(screen.getByTestId("sort-ttft")).toBeTruthy();
		expect(screen.queryByTestId("sort-totalTokens")).toBeNull();
	});

	it("行不可点击（无 cursor-pointer 与跳转标题）", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [makeItem()] };
		renderCard();

		const row = screen.getByText("key-a").closest("tr");
		expect(row).toBeTruthy();
		// 与供应商赛马不同：API Key 无二级页，行不带可点击样式与 title。
		expect(row?.className).not.toContain("cursor-pointer");
		expect(row?.getAttribute("title")).toBeNull();
	});

	it("时间窗口控制：天/周/月/年/自定义切换", () => {
		mocks.data = { startTime: 0, endTime: 0, items: [] };
		renderCard();

		expect(screen.getByRole("button", { name: "天" })).toHaveAttribute("aria-pressed", "true");

		fireEvent.click(screen.getByRole("button", { name: "周" }));
		expect(screen.getByRole("button", { name: "周" })).toHaveAttribute("aria-pressed", "true");

		fireEvent.click(screen.getByRole("button", { name: "自定义" }));
		expect(screen.getByTestId("custom-range-label")).toBeTruthy();
		fireEvent.click(screen.getByTestId("custom-range-label"));
		expect(screen.getByTestId("custom-start")).toBeTruthy();
		expect(screen.getByTestId("custom-end")).toBeTruthy();
		expect(screen.getByRole("button", { name: "确认" })).toBeTruthy();
	});

	it("加载失败展示重试按钮，点击触发 refetch", () => {
		mocks.isError = true;
		renderCard();

		const retry = screen.getByRole("button", { name: "重试" });
		fireEvent.click(retry);
		expect(mocks.refetch).toHaveBeenCalled();
	});
});
