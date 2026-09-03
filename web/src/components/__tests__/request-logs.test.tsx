import { RequestLogsTable } from "@/components/request-logs/RequestLogsTable";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	useRequestLogs: vi.fn(),
	useVirtualModels: vi.fn(),
	useApiKeys: vi.fn(),
	useProviders: vi.fn(),
	useProviderModels: vi.fn(),
	refetch: vi.fn(),
}));

vi.mock("@/hooks/use-request-logs", () => ({
	useRequestLogs: mocks.useRequestLogs,
}));
vi.mock("@/hooks/use-virtual-models", () => ({
	useVirtualModels: mocks.useVirtualModels,
}));
vi.mock("@/hooks/use-api-keys", () => ({
	useApiKeys: mocks.useApiKeys,
}));
vi.mock("@/hooks/use-providers", () => ({
	useProviders: mocks.useProviders,
	useProviderDetail: (id: number | null) => ({
		data: id ? { id, name: "Provider Beta" } : undefined,
	}),
}));
vi.mock("@/hooks/use-provider-models", () => ({
	useProviderModels: mocks.useProviderModels,
}));

function makeRow(overrides: Partial<Parameters<typeof mocks.useRequestLogs>[0]> = {}) {
	return {
		requestId: "req-1",
		virtualModelId: 1,
		virtualModelDisplayId: "vm-a",
		providerId: 2,
		providerName: "Provider Beta",
		modelId: "gpt-4o",
		stream: false,
		ttft: null,
		inputTokens: 100,
		inputCacheTokens: 0,
		inputCacheRate: 0,
		outputTokens: 50,
		outputTokensTime: 500,
		tps: 100,
		startTime: 1700000000000,
		endTime: 1700000001000,
		requestTime: 1000,
		success: true,
		failReason: null,
		totalTokens: 150,
		apiKeyName: "key-a",
		...overrides,
	};
}

function makeProviderModel(modelId: number, providerId: number, providerModelId: string) {
	return {
		modelId,
		providerId,
		providerModelId,
		contextLength: 0,
		maxOutputTokens: 0,
		reasoning: false,
		toolUse: false,
		imageUnderstand: false,
		videoUnderstand: false,
		createdAt: "",
		updatedAt: "",
	};
}

function mockQuery(data: { items: ReturnType<typeof makeRow>[]; total: number }) {
	mocks.useRequestLogs.mockReturnValue({
		data,
		isLoading: false,
		isError: false,
		refetch: mocks.refetch,
	});
}

describe("RequestLogsTable", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		window.localStorage.clear();
		mocks.useVirtualModels.mockReturnValue({
			data: [
				{ virtualModelId: 1, displayId: "vm-a" },
				{ virtualModelId: 2, displayId: "vm-b" },
			],
		});
		mocks.useApiKeys.mockReturnValue({
			data: [{ id: 1, name: "key-a" }],
		});
		mocks.useProviders.mockReturnValue({
			data: [{ id: 2, name: "Provider Beta" }],
		});
		mocks.useProviderModels.mockReturnValue({
			data: [makeProviderModel(10, 2, "gpt-4o")],
		});
	});

	it("渲染行数据并可打开详情弹窗展示全部字段", async () => {
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);

		expect(screen.getByText("vm-a")).toBeInTheDocument();
		expect(screen.getByText("Provider Beta")).toBeInTheDocument();
		expect(screen.getByText("key-a")).toBeInTheDocument();

		// 点击行打开详情弹窗。
		fireEvent.click(screen.getByText("vm-a"));
		await waitFor(() => expect(screen.getByText(/请求 req-1/)).toBeInTheDocument());
		const dialog = screen.getByRole("dialog");
		// 弹窗包含失败原因等字段。
		expect(within(dialog).getByText("首 token 耗时 (TTFT)")).toBeInTheDocument();
		expect(within(dialog).getByText("缓存命中率")).toBeInTheDocument();
		// 供应商名称通过详情接口查询展示（限定弹窗内，避免与表格新列文本混淆）。
		expect(within(dialog).getAllByText("Provider Beta").length).toBeGreaterThan(0);
	});

	it("供应商列缺失名称时兜底显示 #providerId", () => {
		// providerName 为 null（对应供应商已删除）：表格行应显示 #id 而非空。
		mockQuery({ items: [makeRow({ providerName: null })], total: 1 });
		render(<RequestLogsTable />);

		expect(screen.getByText("#2")).toBeInTheDocument();
	});

	it("空数据显示空态", () => {
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);
		expect(screen.getByText("暂无请求日志")).toBeInTheDocument();
	});

	it("重置按钮清空过滤并回第一页", () => {
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);

		// 先勾选一个虚拟模型（部分选择），再重置。
		fireEvent.click(screen.getByRole("button", { name: "按虚拟模型过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "vm-a" }));

		const reset = screen.getByRole("button", { name: /重置/ });
		fireEvent.click(reset);
		// 虚拟模型过滤回到「全部」。
		const vmCombobox = screen.getByRole("button", { name: "按虚拟模型过滤" });
		expect(vmCombobox).toHaveTextContent("全部");
		const calls = mocks.useRequestLogs.mock.calls;
		expect(calls[calls.length - 1]?.[0]?.vmId).toBeUndefined();
	});

	it("默认时间组件选中「天」，重置后回到「天」", () => {
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		// 默认「天」处于按下态。
		const dayButton = screen.getByRole("button", { name: "天" });
		expect(dayButton).toHaveAttribute("aria-pressed", "true");

		// 切到「周」再重置，回到「天」。
		fireEvent.click(screen.getByRole("button", { name: "周" }));
		expect(screen.getByRole("button", { name: "周" })).toHaveAttribute("aria-pressed", "true");

		fireEvent.click(screen.getByRole("button", { name: /重置/ }));
		expect(screen.getByRole("button", { name: "天" })).toHaveAttribute("aria-pressed", "true");
	});

	it("时间过滤通过 startTime/endTime 传给请求", () => {
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		// 默认「天」：startTime = 今天 0 点，endTime = 当前时刻。
		const params = mocks.useRequestLogs.mock.calls[0]?.[0];
		expect(params).toBeDefined();
		const start = new Date(params?.startTime ?? 0);
		expect(start.getHours()).toBe(0);
		expect(start.getMinutes()).toBe(0);
		expect(typeof params.endTime).toBe("number");
	});

	it("重置后 endTime 刷新为新的当前时刻（修复固化 now 导致查不到最新日志）", () => {
		vi.useFakeTimers();
		vi.setSystemTime(new Date(2026, 7, 31, 10, 0, 0));
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		const firstEnd = mocks.useRequestLogs.mock.calls[0]?.[0]?.endTime;
		expect(firstEnd).toBe(new Date(2026, 7, 31, 10, 0, 0).getTime());

		// 时间前进 5 分钟后重置：endTime 应更新到新时刻。
		vi.setSystemTime(new Date(2026, 7, 31, 10, 5, 0));
		fireEvent.click(screen.getByRole("button", { name: /重置/ }));

		const calls = mocks.useRequestLogs.mock.calls;
		const lastCall = calls[calls.length - 1]?.[0];
		expect(lastCall?.endTime).toBe(new Date(2026, 7, 31, 10, 5, 0).getTime());
		expect(lastCall?.endTime).toBeGreaterThan(firstEnd ?? 0);

		vi.useRealTimers();
	});

	it("localStorage 无数据时默认全部列 + 每页 20", () => {
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);

		// 默认每页 20 条。
		const lastCall = mocks.useRequestLogs.mock.calls[0]?.[0];
		expect(lastCall?.pageSize).toBe(20);
		// 列显隐为空对象 → 全部列显示（表头齐全；过滤卡片有同名 label 故用 getAll）。
		expect(screen.getAllByText("虚拟模型").length).toBeGreaterThan(0);
		expect(screen.getAllByText("供应商").length).toBeGreaterThan(0);
		expect(screen.getAllByText("API Key").length).toBeGreaterThan(0);
		expect(screen.getAllByText("上游模型").length).toBeGreaterThan(0);
	});

	it("勾选隐藏列后写入 localStorage，重新渲染保持隐藏", () => {
		mockQuery({ items: [makeRow()], total: 1 });
		const { unmount } = render(<RequestLogsTable />);

		// Radix DropdownMenu 在 jsdom 下通过键盘事件打开。
		fireEvent.keyDown(screen.getByRole("button", { name: /显示列/ }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "结果" }));

		const stored = JSON.parse(
			window.localStorage.getItem("request-logs:column-visibility") ?? "{}",
		);
		expect(stored).toEqual({ success: false });

		// 重新渲染（模拟刷新）：结果列保持隐藏。
		unmount();
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);
		fireEvent.keyDown(screen.getByRole("button", { name: /显示列/ }), { key: "ArrowDown" });
		expect(screen.getByRole("menuitemcheckbox", { name: "结果" })).toHaveAttribute(
			"data-state",
			"unchecked",
		);
	});

	it("勾选隐藏供应商列后写入 localStorage，重新渲染保持隐藏", () => {
		mockQuery({ items: [makeRow()], total: 1 });
		const { unmount } = render(<RequestLogsTable />);

		// Radix DropdownMenu 在 jsdom 下通过键盘事件打开。
		fireEvent.keyDown(screen.getByRole("button", { name: /显示列/ }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "供应商" }));

		const stored = JSON.parse(
			window.localStorage.getItem("request-logs:column-visibility") ?? "{}",
		);
		expect(stored).toEqual({ providerName: false });

		// 重新渲染（模拟刷新）：供应商列保持隐藏。
		unmount();
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);
		fireEvent.keyDown(screen.getByRole("button", { name: /显示列/ }), { key: "ArrowDown" });
		expect(screen.getByRole("menuitemcheckbox", { name: "供应商" })).toHaveAttribute(
			"data-state",
			"unchecked",
		);
	});

	it("切换每页条数后写入 localStorage，重新渲染保持", () => {
		mockQuery({ items: [], total: 0 });
		const { unmount } = render(<RequestLogsTable />);

		// 每页切到 10 条。
		fireEvent.click(screen.getByRole("combobox", { name: "每页条数" }));
		fireEvent.click(screen.getByRole("option", { name: "10 / 页" }));
		expect(window.localStorage.getItem("request-logs:page-size")).toBe("10");

		// 重新渲染（模拟刷新）：每页仍 10 条。
		unmount();
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);
		const calls = mocks.useRequestLogs.mock.calls;
		const lastCall = calls[calls.length - 1]?.[0];
		expect(lastCall?.pageSize).toBe(10);
	});

	it("上游模型下拉按供应商分组，同名模型分属各自供应商组", () => {
		mocks.useProviders.mockReturnValue({
			data: [
				{ id: 2, name: "Provider Beta" },
				{ id: 3, name: "Provider Alpha" },
			],
		});
		mocks.useProviderModels.mockReturnValue({
			data: [
				makeProviderModel(10, 2, "gpt-4o"),
				makeProviderModel(11, 3, "gpt-4o"),
				makeProviderModel(12, 3, "gemini-2.5-pro"),
			],
		});
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		fireEvent.click(screen.getByRole("button", { name: "按供应商模型过滤" }));
		// 分组标题按供应商列表顺序展示。
		const labels = screen.getAllByText(/^Provider (Beta|Alpha)$/);
		expect(labels.map((l) => l.textContent)).toEqual(["Provider Beta", "Provider Alpha"]);
		// 同名模型在两个供应商组下各出现一次。
		expect(screen.getAllByRole("checkbox", { name: "gpt-4o" })).toHaveLength(2);
		expect(screen.getByRole("checkbox", { name: "gemini-2.5-pro" })).toBeInTheDocument();
	});

	it("选中供应商后模型下拉只剩该供应商分组", () => {
		mocks.useProviders.mockReturnValue({
			data: [
				{ id: 2, name: "Provider Beta" },
				{ id: 3, name: "Provider Alpha" },
			],
		});
		mocks.useProviderModels.mockReturnValue({
			data: [
				makeProviderModel(10, 2, "gpt-4o"),
				makeProviderModel(11, 3, "gpt-4o"),
				makeProviderModel(13, 3, "claude-3"),
			],
		});
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		// 「全部」态下点 Provider Beta → 退化为只选 Beta。
		fireEvent.click(screen.getByRole("button", { name: "按供应商过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "Provider Beta" }));
		// 点击模型触发按钮：供应商弹层经外部点击关闭，模型弹层打开。
		fireEvent.click(screen.getByRole("button", { name: "按供应商模型过滤" }));
		// 模型弹层只应剩 Provider Beta 分组的模型；供应商触发器显示 Provider Beta。
		expect(screen.queryAllByText("Provider Alpha")).toHaveLength(0);
		expect(screen.getAllByRole("checkbox", { name: "gpt-4o" })).toHaveLength(1);
		expect(screen.queryByRole("checkbox", { name: "claude-3" })).not.toBeInTheDocument();
		expect(screen.getByRole("button", { name: "按供应商过滤" })).toHaveTextContent("Provider Beta");
	});

	it("「全部」态点虚拟模型退化为仅选该项，过滤参数为该项", () => {
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		// 初始为「全部」，点 vm-b 退化为只选 vm-b。
		fireEvent.click(screen.getByRole("button", { name: "按虚拟模型过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "vm-b" }));

		const calls = mocks.useRequestLogs.mock.calls;
		expect(calls[calls.length - 1]?.[0]?.vmId).toEqual([2]);
		expect(screen.getByRole("button", { name: "按虚拟模型过滤" })).toHaveTextContent("vm-b");
	});

	it("勾满全部选项归一化为「全部」，不传过滤参数", () => {
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		fireEvent.click(screen.getByRole("button", { name: "按虚拟模型过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "vm-a" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "vm-b" }));

		const calls = mocks.useRequestLogs.mock.calls;
		expect(calls[calls.length - 1]?.[0]?.vmId).toBeUndefined();
		expect(screen.getByRole("button", { name: "按虚拟模型过滤" })).toHaveTextContent("全部");
	});

	it("多选供应商后模型下拉含各供应商模型（并集，未全选不归一化）", async () => {
		mocks.useProviders.mockReturnValue({
			data: [
				{ id: 1, name: "Provider Gamma" },
				{ id: 2, name: "Provider Beta" },
				{ id: 3, name: "Provider Alpha" },
			],
		});
		mocks.useProviderModels.mockReturnValue({
			data: [
				makeProviderModel(10, 2, "gpt-4o"),
				makeProviderModel(11, 3, "gpt-4o"),
				makeProviderModel(13, 3, "claude-3"),
			],
		});
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		// 供应商「全部」态点 Provider Alpha → 退化为只选 Alpha。
		fireEvent.click(screen.getByRole("button", { name: "按供应商过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "Provider Alpha" }));
		// 再选 Provider Beta（显式态新增）→ [Beta, Alpha]，Gamma 未选故不归一化。
		fireEvent.click(screen.getByRole("checkbox", { name: "Provider Beta" }));
		expect(screen.getByRole("button", { name: "按供应商过滤" })).toHaveTextContent("已选 2 项");

		// 模型下拉为两供应商模型并集：两个 gpt-4o + claude-3。
		fireEvent.click(screen.getByRole("button", { name: "按供应商模型过滤" }));
		expect(screen.getAllByRole("checkbox", { name: "gpt-4o" })).toHaveLength(2);
		expect(screen.getByRole("checkbox", { name: "claude-3" })).toBeInTheDocument();
	});

	it("换选供应商后失效的已选模型被剔除", async () => {
		mocks.useProviders.mockReturnValue({
			data: [
				{ id: 2, name: "Provider Beta" },
				{ id: 3, name: "Provider Alpha" },
			],
		});
		mocks.useProviderModels.mockReturnValue({
			data: [
				makeProviderModel(10, 2, "gpt-4o"),
				makeProviderModel(11, 3, "gpt-4o"),
				makeProviderModel(13, 3, "claude-3"),
			],
		});
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);

		// 供应商「全部」态点 Provider Alpha → 只选 Alpha。
		fireEvent.click(screen.getByRole("button", { name: "按供应商过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "Provider Alpha" }));
		// 模型下拉只剩 Alpha 组；点 claude-3 → 只选 claude-3（pk 13）。
		fireEvent.click(screen.getByRole("button", { name: "按供应商模型过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "claude-3" }));

		// 换选供应商：供应商弹层取消 Provider Alpha → 回「全部」，再点 Provider Beta → 只选 Beta。
		fireEvent.click(screen.getByRole("button", { name: "按供应商过滤" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "Provider Alpha" }));
		fireEvent.click(screen.getByRole("checkbox", { name: "Provider Beta" }));
		expect(screen.getByRole("button", { name: "按供应商过滤" })).toHaveTextContent("Provider Beta");

		// 模型选项只剩 Beta 的 gpt-4o（pk 10），失效的 claude-3 被剔除，模型回到全部。
		await waitFor(() =>
			expect(screen.queryByRole("checkbox", { name: "claude-3" })).not.toBeInTheDocument(),
		);
		const calls = mocks.useRequestLogs.mock.calls;
		expect(calls[calls.length - 1]?.[0]?.modelId).toBeUndefined();
		expect(screen.getByRole("button", { name: "按供应商模型过滤" })).toHaveTextContent("全部");
	});
});
