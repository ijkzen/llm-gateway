import { AddProviderModelsDialog } from "@/components/provider-models/AddProviderModelsDialog";
import { ProviderModelDetailDialog } from "@/components/provider-models/ProviderModelDetailDialog";
import type { ProviderModel, RefreshCandidate } from "@/hooks/use-provider-models";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		refreshMutate: vi.fn(),
		batchMutate: vi.fn(),
		createMutate: vi.fn(),
		updateMutate: vi.fn(),
		deleteMutate: vi.fn(),
	};
});

vi.mock("@/hooks/use-provider-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-provider-models")>(
		"@/hooks/use-provider-models",
	);
	return {
		...actual,
		useRefreshProviderModels: () => ({ mutate: mocks.refreshMutate, isPending: false }),
		useBatchCreateProviderModels: () => ({ mutate: mocks.batchMutate, isPending: false }),
		useCreateProviderModel: () => ({ mutate: mocks.createMutate, isPending: false }),
		useUpdateProviderModel: () => ({ mutate: mocks.updateMutate, isPending: false }),
		useDeleteProviderModel: () => ({ mutate: mocks.deleteMutate, isPending: false }),
		useCatalogSearch: () => ({ data: [] }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: vi.fn(),
		toastError: vi.fn(),
	}),
}));

function makeCandidate(overrides: Partial<RefreshCandidate> = {}): RefreshCandidate {
	return {
		providerModelId: "gpt-4o",
		matchState: "smart",
		contextLength: 128000,
		maxOutputTokens: 4096,
		reasoning: true,
		toolUse: true,
		imageUnderstand: false,
		videoUnderstand: false,
		...overrides,
	};
}

function makeModel(overrides: Partial<ProviderModel> = {}): ProviderModel {
	return {
		modelId: 1,
		providerId: 7,
		providerModelId: "gpt-4o",
		contextLength: 128000,
		maxOutputTokens: 4096,
		reasoning: true,
		toolUse: false,
		imageUnderstand: false,
		videoUnderstand: false,
		createdAt: "2026-08-29T00:00:00Z",
		updatedAt: "2026-08-29T00:00:00Z",
		...overrides,
	};
}

const provider = { id: 7, name: "OpenAI" } as never;

/** 收窄可能为 undefined 的元素/数组项（tsc 开启 noUncheckedIndexedAccess）。 */
function required<T>(value: T | undefined | null): T {
	if (value === undefined || value === null) {
		throw new Error("required element/value not found");
	}
	return value;
}

beforeEach(() => {
	vi.clearAllMocks();
});

describe("AddProviderModelsDialog 候选三态", () => {
	it("刷新成功后渲染候选卡片：smart 可勾选、manual 禁选；全部不预选", () => {
		mocks.refreshMutate.mockImplementation((_args, opts) => {
			opts.onSuccess([
				makeCandidate(),
				makeCandidate({
					providerModelId: "secret-embedding",
					matchState: "manual",
					contextLength: null,
					maxOutputTokens: null,
					reasoning: false,
					toolUse: false,
				}),
			]);
		});

		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);
		fireEvent.click(screen.getByRole("button", { name: "尝试刷新" }));

		expect(screen.getByText("已智能填充")).toBeTruthy();
		expect(screen.getByText("需手动填写")).toBeTruthy();

		const smartSwitch = screen.getByRole("checkbox", { name: "选择 gpt-4o" });
		const manualSwitch = screen.getByRole("checkbox", { name: "选择 secret-embedding" });
		// Radix Checkbox 渲染为 button[role=checkbox]，用 aria-checked 表达选中态。
		expect((smartSwitch as HTMLButtonElement).disabled).toBe(false);
		expect((manualSwitch as HTMLButtonElement).disabled).toBe(true);
		expect(smartSwitch.getAttribute("aria-checked")).toBe("false");
		expect(manualSwitch.getAttribute("aria-checked")).toBe("false");
	});

	it("partial 状态显示黄色提示，预填已有数字，缺失时禁选", () => {
		mocks.refreshMutate.mockImplementation((_args, opts) => {
			opts.onSuccess([
				makeCandidate({
					providerModelId: "trendyol/asure-12b",
					matchState: "partial",
					contextLength: 32768,
					maxOutputTokens: null,
					reasoning: false,
					toolUse: false,
				}),
			]);
		});

		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);
		fireEvent.click(screen.getByRole("button", { name: "尝试刷新" }));

		expect(screen.getByText("信息不完整")).toBeTruthy();
		const inputs = screen.getAllByRole("spinbutton");
		expect((inputs[0] as HTMLInputElement).value).toBe("32768");
		expect((inputs[1] as HTMLInputElement).value).toBe("");
		const checkbox = screen.getByRole("checkbox", { name: "选择 trendyol/asure-12b" });
		expect((checkbox as HTMLButtonElement).disabled).toBe(true);
	});

	it("补齐缺失数字后解锁勾选，勾选并点击添加触发批量导入", () => {
		mocks.refreshMutate.mockImplementation((_args, opts) => {
			opts.onSuccess([
				makeCandidate({
					providerModelId: "secret-embedding",
					matchState: "manual",
					contextLength: null,
					maxOutputTokens: null,
					reasoning: false,
					toolUse: false,
				}),
			]);
		});

		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);
		fireEvent.click(screen.getByRole("button", { name: "尝试刷新" }));

		const checkbox = screen.getByRole("checkbox", { name: "选择 secret-embedding" });
		expect((checkbox as HTMLButtonElement).disabled).toBe(true);

		const inputs = screen.getAllByRole("spinbutton");
		fireEvent.change(required(inputs[0]), { target: { value: "8192" } });
		fireEvent.change(required(inputs[1]), { target: { value: "1024" } });
		expect((checkbox as HTMLButtonElement).disabled).toBe(false);

		fireEvent.click(checkbox);
		fireEvent.click(screen.getByRole("button", { name: "添加" }));

		expect(mocks.batchMutate).toHaveBeenCalledTimes(1);
		const payload = required(mocks.batchMutate.mock.calls[0])[0];
		expect(payload.models).toEqual([
			{
				providerModelId: "secret-embedding",
				contextLength: 8192,
				maxOutputTokens: 1024,
				reasoning: false,
				toolUse: false,
				imageUnderstand: false,
				videoUnderstand: false,
			},
		]);
	});
});

describe("AddProviderModelsDialog 手动添加", () => {
	it("manual 候选卡片点击后预填模型 ID 并滚动到手动表单", async () => {
		mocks.refreshMutate.mockImplementation((_args, opts) => {
			opts.onSuccess([
				makeCandidate({
					providerModelId: "manual/only-model",
					matchState: "manual",
					contextLength: null,
					maxOutputTokens: null,
					reasoning: false,
					toolUse: false,
				}),
			]);
		});
		// scrollIntoView 在 jsdom 未实现，打桩避免报错。
		Element.prototype.scrollIntoView = vi.fn();

		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);
		fireEvent.click(screen.getByRole("button", { name: "尝试刷新" }));

		const input = screen.getByPlaceholderText("如 gpt-4o") as HTMLInputElement;
		expect(input.value).toBe("");

		fireEvent.click(screen.getByText("manual/only-model"));

		expect(input.value).toBe("manual/only-model");
		expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
		await waitFor(() => expect(document.activeElement).toBe(input));
	});

	it("填写完整后提交触发单条创建", async () => {
		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);

		fireEvent.change(screen.getByPlaceholderText("如 gpt-4o"), {
			target: { value: "my-model" },
		});
		const numbers = screen.getAllByRole("spinbutton");
		fireEvent.change(required(numbers[0]), { target: { value: "8192" } });
		fireEvent.change(required(numbers[1]), { target: { value: "2048" } });
		fireEvent.click(screen.getByRole("button", { name: "手动添加" }));

		await waitFor(() => expect(mocks.createMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.createMutate.mock.calls[0])[0];
		expect(payload.providerModelId).toBe("my-model");
		expect(payload.contextLength).toBe(8192);
		expect(payload.maxOutputTokens).toBe(2048);
	});

	it("模型 ID 为空时不提交", () => {
		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);

		const numbers = screen.getAllByRole("spinbutton");
		fireEvent.change(required(numbers[0]), { target: { value: "8192" } });
		fireEvent.change(required(numbers[1]), { target: { value: "2048" } });
		fireEvent.click(screen.getByRole("button", { name: "手动添加" }));

		expect(mocks.createMutate).not.toHaveBeenCalled();
	});
});

describe("ProviderModelDetailDialog 编辑态", () => {
	it("默认只读：只有编辑按钮，没有删除/更新", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		expect(screen.getByRole("button", { name: "编辑" })).toBeTruthy();
		expect(screen.queryByRole("button", { name: "删除" })).toBeNull();
		expect(screen.queryByRole("button", { name: "更新" })).toBeNull();
		expect(screen.getByText("上下文长度")).toBeTruthy();
		expect(screen.getByText("128,000")).toBeTruthy();
	});

	it("点击编辑后右上角变为删除与更新，编辑消失", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "编辑" }));

		expect(screen.queryByRole("button", { name: "编辑" })).toBeNull();
		expect(screen.getByRole("button", { name: "删除" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "更新" })).toBeTruthy();
	});

	it("更新提交后调用更新接口", async () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "编辑" }));
		const numbers = screen.getAllByRole("spinbutton");
		fireEvent.change(required(numbers[0]), { target: { value: "256000" } });
		fireEvent.click(screen.getByRole("button", { name: "更新" }));

		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.updateMutate.mock.calls[0])[0];
		expect(payload.modelId).toBe(1);
		expect(payload.contextLength).toBe(256000);
		expect(payload.maxOutputTokens).toBe(4096);
	});
});
