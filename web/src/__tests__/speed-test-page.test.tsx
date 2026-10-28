import type { Provider } from "@/hooks/use-providers";
import type { CandidatePreview, SpeedTestRunData } from "@/hooks/use-speed-test";
import type { VirtualModel } from "@/hooks/use-virtual-models";
import SpeedTestPage from "@/pages/speed-test";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		providers: [] as Provider[],
		virtualModels: [] as VirtualModel[],
		candidates: undefined as CandidatePreview | undefined,
		runData: undefined as SpeedTestRunData | undefined,
		isLoading: false,
		isError: false,
		startMutate: vi.fn(),
		cancelMutate: vi.fn(),
		refetch: vi.fn(),
	};
});

function provider(id: number, name: string, enable = true): Provider {
	return {
		id,
		name,
		enable,
		baseUrl: "https://example.com",
		apiKeyMasked: "sk-***",
		protocolType: 0,
		billingMode: 0,
		customHeader: "{}",
		extra: "{}",
		proxyEnabled: false,
		proxyAddr: "",
		createdAt: "",
		updatedAt: "",
	};
}

function virtualModel(id: number, displayId: string): VirtualModel {
	return {
		virtualModelId: id,
		displayId,
		enable: true,
		loadBalancingStrategy: 0,
		fallbackStrategy: 0,
		interfaceType: 0,
		items: [],
		createdAt: "",
		updatedAt: "",
	};
}

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return { ...actual, useProviders: () => ({ data: mocks.providers }) };
});

vi.mock("@/hooks/use-virtual-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-virtual-models")>(
		"@/hooks/use-virtual-models",
	);
	return { ...actual, useVirtualModels: () => ({ data: mocks.virtualModels }) };
});

vi.mock("@/hooks/use-speed-test", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-speed-test")>("@/hooks/use-speed-test");
	return {
		...actual,
		useSpeedTestCandidates: () => ({ data: mocks.candidates }),
		useSpeedTestRun: () => ({
			data: mocks.runData,
			isLoading: mocks.isLoading,
			isError: mocks.isError,
			refetch: mocks.refetch,
		}),
		useStartSpeedTest: () => ({ mutate: mocks.startMutate, isPending: false }),
		useCancelSpeedTest: () => ({ mutate: mocks.cancelMutate, isPending: false }),
	};
});

function renderPage() {
	return render(
		<MemoryRouter initialEntries={["/speed-test"]}>
			<SpeedTestPage />
		</MemoryRouter>,
	);
}

/** 取非空元素：断言与取数合并，避免 `rows[0]` 在严格索引检查下为 undefined。 */
function required<T>(value: T | undefined): T {
	if (value === undefined) throw new Error("required element not found");
	return value;
}

describe("speed-test 页面", () => {
	beforeEach(() => {
		mocks.providers = [provider(1, "云雾"), provider(2, "停用供应商", false)];
		mocks.virtualModels = [virtualModel(1, "gpt-4o-prod")];
		mocks.candidates = {
			total: 3,
			providers: [{ providerId: 1, providerName: "云雾", count: 3 }],
			models: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "gpt-4o",
					virtualModels: ["gpt-4o-prod"],
				},
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 11,
					modelId: "claude-sonnet",
					virtualModels: [],
				},
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 12,
					modelId: "qwen-max",
					virtualModels: ["qwen-prod"],
				},
			],
		};
		mocks.runData = { run: null, results: [] };
		mocks.isLoading = false;
		mocks.isError = false;
		mocks.startMutate.mockReset();
		mocks.cancelMutate.mockReset();
		mocks.refetch.mockReset();
	});

	it("无运行记录时直接展示候选模型清单（不必等测速）", () => {
		renderPage();
		expect(screen.getByTestId("speed-test-candidates-hint").textContent).toContain("3");
		expect(screen.getByText("gpt-4o")).toBeTruthy();
		expect(screen.getByText("claude-sonnet")).toBeTruthy();
		expect(screen.getByText("qwen-max")).toBeTruthy();
		// 虚拟模型引用与「未被引用」都如实展示。
		expect(screen.getByText("gpt-4o-prod")).toBeTruthy();
		expect(screen.getByText("未被虚拟模型引用")).toBeTruthy();
	});

	it("候选为空时展示「没有符合条件的模型」", () => {
		mocks.candidates = { total: 0, providers: [], models: [] };
		renderPage();
		expect(screen.getByText("没有符合条件的模型")).toBeTruthy();
	});

	it("改了筛选后不再展示上一批成绩，而是展示新候选清单", () => {
		// 已有一次运行（筛选为空），但 URL 上带 providerIds=1，与运行快照不符。
		mocks.runData = {
			run: {
				id: 9,
				status: "completed",
				startedAt: 1_700_000_000_000,
				finishedAt: 1_700_000_100_000,
				total: 1,
				running: false,
				providerIds: [],
				vmIds: [],
				keyword: "",
			},
			results: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "stale-model",
					status: "done",
					tps: 99,
					outputTokens: 2048,
					requestId: "speedtest-stale",
					finishedAt: 1_700_000_050_000,
				},
			],
		};
		render(
			<MemoryRouter initialEntries={["/speed-test?providerIds=1"]}>
				<SpeedTestPage />
			</MemoryRouter>,
		);
		expect(screen.queryByText("stale-model")).toBeNull();
		expect(screen.getByTestId("speed-test-candidates-hint")).toBeTruthy();
		expect(screen.getByText("claude-sonnet")).toBeTruthy();
	});

	it("筛选与运行快照一致时展示成绩", () => {
		mocks.runData = {
			run: {
				id: 10,
				status: "completed",
				startedAt: 1_700_000_000_000,
				finishedAt: 1_700_000_100_000,
				total: 1,
				running: false,
				providerIds: [1],
				vmIds: [],
				keyword: "",
			},
			results: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "fresh-model",
					status: "done",
					tps: 42,
					outputTokens: 2048,
					requestId: "speedtest-fresh",
					finishedAt: 1_700_000_050_000,
				},
			],
		};
		render(
			<MemoryRouter initialEntries={["/speed-test?providerIds=1"]}>
				<SpeedTestPage />
			</MemoryRouter>,
		);
		expect(screen.getByText("fresh-model")).toBeTruthy();
		expect(screen.queryByTestId("speed-test-candidates-hint")).toBeNull();
	});

	it("候选计数与开始按钮渲染，按钮点击带上过滤条件", () => {
		renderPage();
		expect(screen.getByText("将测 3 个模型")).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: /开始测速/ }));
		expect(mocks.startMutate).toHaveBeenCalledTimes(1);
		const firstCall = mocks.startMutate.mock.calls[0];
		expect(firstCall).toBeDefined();
		const filter = firstCall?.[0];
		expect(filter).toEqual({ providerIds: [], vmIds: [], keyword: "" });
	});

	it("候选为空时开始按钮禁用", () => {
		mocks.candidates = { total: 0, providers: [], models: [] };
		renderPage();
		const button = screen.getByRole("button", { name: /开始测速/ }) as HTMLButtonElement;
		expect(button.disabled).toBe(true);
	});

	it("运行中展示进度与取消按钮", () => {
		mocks.runData = {
			run: {
				id: 1,
				status: "running",
				startedAt: 1_700_000_000_000,
				finishedAt: null,
				total: 2,
				running: true,
				providerIds: [],
				vmIds: [],
				keyword: "",
			},
			results: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "gpt-4o",
					status: "done",
					tps: 42.5,
					outputTokens: 2048,
					ttftMs: 300,
					totalMs: 48_000,
					requestId: "speedtest-abc",
					finishedAt: 1_700_000_001_000,
				},
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 11,
					modelId: "claude",
					status: "pending",
				},
			],
		};
		renderPage();

		expect(screen.getByRole("button", { name: /取消测速/ })).toBeTruthy();
		expect(screen.queryByRole("button", { name: /开始测速/ })).toBeNull();
		expect(screen.getByTestId("speed-test-progress").textContent).toContain("1 / 2");

		fireEvent.click(screen.getByRole("button", { name: /取消测速/ }));
		expect(mocks.cancelMutate).toHaveBeenCalledTimes(1);
	});

	it("完成后展示成绩，失败行沉底且带失败原因", () => {
		mocks.runData = {
			run: {
				id: 2,
				status: "completed",
				startedAt: 1_700_000_000_000,
				finishedAt: 1_700_000_100_000,
				total: 3,
				running: false,
				providerIds: [],
				vmIds: [],
				keyword: "",
			},
			results: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "slow-model",
					status: "done",
					tps: 8.2,
					outputTokens: 2048,
					ttftMs: 1200,
					totalMs: 250_000,
					requestId: "speedtest-slow",
					finishedAt: 1_700_000_050_000,
				},
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 11,
					modelId: "fast-model",
					status: "done",
					tps: 55.1,
					outputTokens: 2048,
					ttftMs: 200,
					totalMs: 38_000,
					requestId: "speedtest-fast",
					finishedAt: 1_700_000_060_000,
				},
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 12,
					modelId: "broken-model",
					status: "failed",
					failReason: "500 upstream boom",
					requestId: "speedtest-bad",
					finishedAt: 1_700_000_070_000,
				},
			],
		};
		renderPage();

		const table = screen.getByRole("table");
		const rows = within(table).getAllByRole("row").slice(1);
		// 默认 TPS 降序：fast（55.1）→ slow（8.2）→ 失败沉底。
		const [firstRow, secondRow, thirdRow] = rows;
		expect(within(required(firstRow)).getByText("fast-model")).toBeTruthy();
		expect(within(required(secondRow)).getByText("slow-model")).toBeTruthy();
		expect(within(required(thirdRow)).getByText("broken-model")).toBeTruthy();
		expect(within(required(thirdRow)).getByText("500 upstream boom")).toBeTruthy();

		// 汇总条：算术平均 TPS = (8.2 + 55.1) / 2 = 31.65
		const summary = screen.getByTestId("speed-test-summary");
		expect(summary.textContent).toContain("31.65");
		expect(summary.textContent).toContain("成功 2");
		expect(summary.textContent).toContain("失败 1");
	});

	it("输出 token 偏少时标「样本不足」", () => {
		mocks.runData = {
			run: {
				id: 3,
				status: "completed",
				startedAt: 1_700_000_000_000,
				finishedAt: 1_700_000_100_000,
				total: 1,
				running: false,
				providerIds: [],
				vmIds: [],
				keyword: "",
			},
			results: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "short-model",
					status: "done",
					tps: 20,
					outputTokens: 40,
					ttftMs: 100,
					totalMs: 2000,
					requestId: "speedtest-short",
					finishedAt: 1_700_000_050_000,
				},
			],
		};
		renderPage();
		expect(screen.getByText("样本不足")).toBeTruthy();
	});

	it("重启中断的运行展示「已中断」徽标", () => {
		mocks.runData = {
			run: {
				id: 4,
				status: "interrupted",
				startedAt: 1_700_000_000_000,
				finishedAt: 1_700_000_100_000,
				total: 1,
				running: false,
				providerIds: [],
				vmIds: [],
				keyword: "",
			},
			results: [
				{
					providerId: 1,
					providerName: "云雾",
					providerModelPk: 10,
					modelId: "m-1",
					status: "cancelled",
				},
			],
		};
		renderPage();
		expect(screen.getByText("已中断")).toBeTruthy();
		expect(screen.getByText("已取消")).toBeTruthy();
	});

	it("加载失败展示错误空态", () => {
		mocks.isError = true;
		renderPage();
		expect(screen.getByText("无法获取测速结果")).toBeTruthy();
	});
});
