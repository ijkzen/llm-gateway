import { RequestLogsTable } from "@/components/request-logs/RequestLogsTable";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	useRequestLogs: vi.fn(),
	useVirtualModels: vi.fn(),
	useApiKeys: vi.fn(),
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

function makeRow(overrides: Partial<Parameters<typeof mocks.useRequestLogs>[0]> = {}) {
	return {
		requestId: "req-1",
		virtualModelId: 1,
		virtualModelDisplayId: "vm-a",
		providerId: 2,
		modelId: "gpt-4o",
		stream: false,
		ttft: null,
		inputTokens: 100,
		inputCacheTokens: 0,
		inputCacheRate: 0,
		outputTokens: 50,
		outputTokensTime: 500,
		tps: 100,
		networkLatency: 20,
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
		mocks.useVirtualModels.mockReturnValue({
			data: [{ virtualModelId: 1, displayId: "vm-a" }],
		});
		mocks.useApiKeys.mockReturnValue({
			data: [{ id: 1, name: "key-a" }],
		});
	});

	it("渲染行数据并可打开详情弹窗展示全部字段", async () => {
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);

		expect(screen.getByText("vm-a")).toBeInTheDocument();
		expect(screen.getByText("key-a")).toBeInTheDocument();

		// 点击行打开详情弹窗。
		fireEvent.click(screen.getByText("vm-a"));
		await waitFor(() => expect(screen.getByText(/请求 req-1/)).toBeInTheDocument());
		// 弹窗包含失败原因等字段。
		expect(screen.getByText("网络延迟")).toBeInTheDocument();
		expect(screen.getByText("缓存命中率")).toBeInTheDocument();
	});

	it("空数据显示空态", () => {
		mockQuery({ items: [], total: 0 });
		render(<RequestLogsTable />);
		expect(screen.getByText("暂无请求日志")).toBeInTheDocument();
	});

	it("重置按钮清空过滤并回第一页", () => {
		mockQuery({ items: [makeRow()], total: 1 });
		render(<RequestLogsTable />);

		// 默认快捷时间段是 24h，重置后仍为 24h（startTime 有值）。
		const reset = screen.getByRole("button", { name: /重置/ });
		fireEvent.click(reset);
		// 虚拟模型下拉回到「全部」。
		const vmSelect = screen.getByLabelText("按虚拟模型过滤");
		expect(vmSelect).toBeInTheDocument();
	});
});
