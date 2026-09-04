import { AddProviderModelsDialog } from "@/components/provider-models/AddProviderModelsDialog";
import { ProviderModelDetailDialog } from "@/components/provider-models/ProviderModelDetailDialog";
import type { ProviderModel, RefreshCandidate } from "@/hooks/use-provider-models";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		refreshMutate: vi.fn(),
		batchMutate: vi.fn(),
		createMutate: vi.fn(),
		updateMutate: vi.fn(),
		deleteMutate: vi.fn(),
		testMutate: vi.fn(),
		toastSuccess: vi.fn(),
		toastError: vi.fn(),
		testState: { isPending: false },
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
		useTestProviderModel: () => ({
			mutate: mocks.testMutate,
			isPending: mocks.testState.isPending,
		}),
		useCatalogSearch: () => ({ data: [] }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: mocks.toastSuccess,
		toastError: mocks.toastError,
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
		proxyEnabled: false,
		proxyAddr: "",
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

/** 让刷新接口失败并传入给定的错误对象。 */
function failRefresh(message: string) {
	mocks.refreshMutate.mockImplementation((_args, opts) => {
		required(opts).onError(new Error(message));
	});
}

beforeEach(() => {
	vi.clearAllMocks();
	mocks.testState.isPending = false;
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

	it("刷新失败时弹出错误详情弹窗，不再只弹 toast", () => {
		failRefresh("供应商 Models 接口返回 403：<html>…Cloudflare 拦截…</html>");
		render(<AddProviderModelsDialog open onOpenChange={vi.fn()} provider={provider} />);

		fireEvent.click(screen.getByRole("button", { name: "尝试刷新" }));

		// 完整错误内容与引导文案都出现在弹窗中。
		expect(screen.getByText("刷新失败，详情见下方：")).toBeTruthy();
		expect(screen.getByText(/Cloudflare 拦截/)).toBeTruthy();
		expect(mocks.toastError).not.toHaveBeenCalled();
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
				// 批量导入的模型默认关闭模型级代理。
				proxyEnabled: false,
				proxyAddr: "",
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

describe("ProviderModelDetailDialog 测试按钮", () => {
	it("只读态左下角有测试按钮，点击触发测试并传入 modelId", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		const testBtn = screen.getByRole("button", { name: "测试" });
		expect(testBtn).toBeTruthy();
		fireEvent.click(testBtn);
		expect(mocks.testMutate).toHaveBeenCalledTimes(1);
		const [modelId, options] = required(mocks.testMutate.mock.calls[0]);
		expect(modelId).toBe(1);
		required(options).onSuccess();
	});

	it("测试成功弹出成功 toast", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		mocks.testMutate.mockImplementation((_id, opts) => {
			required(opts).onSuccess();
		});
		fireEvent.click(screen.getByRole("button", { name: "测试" }));

		expect(mocks.toastSuccess).toHaveBeenCalledWith("模型测试成功");
	});

	it("测试失败弹出失败弹窗并展示错误信息", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		mocks.testMutate.mockImplementation((_id, opts) => {
			required(opts).onError({ message: "429 rate limited" });
		});
		fireEvent.click(screen.getByRole("button", { name: "测试" }));

		expect(screen.getByText("模型测试失败")).toBeTruthy();
		expect(screen.getByText("429 rate limited")).toBeTruthy();
	});

	it("测试中按钮禁用并显示测试中", () => {
		// isPending 为 true 时按钮禁用。
		mocks.testState.isPending = true;
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		const testBtn = screen.getByRole("button", { name: "测试中..." });
		expect((testBtn as HTMLButtonElement).disabled).toBe(true);
		fireEvent.click(testBtn);
		expect(mocks.testMutate).not.toHaveBeenCalled();
	});
});

describe("ProviderModelDetailDialog 编辑态", () => {
	it("删除成功时先关闭确认弹窗，再关闭模型详情", () => {
		mocks.deleteMutate.mockImplementation((_id, opts) => {
			required(opts).onSuccess();
		});
		let confirmingDialogClosed = false;
		const onOpenChange = (next: boolean) => {
			if (!next) confirmingDialogClosed = screen.queryByRole("alertdialog") === null;
		};
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={onOpenChange}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "编辑" }));
		fireEvent.click(screen.getByRole("button", { name: "删除" }));
		fireEvent.click(screen.getByRole("button", { name: "删除" }));

		expect(confirmingDialogClosed).toBe(true);
	});

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

	it("进入编辑态但未改动任何值时，更新按钮禁用且不触发提交（双击编辑不会立即更新成功）", async () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel()}
			/>,
		);

		// 真实浏览器按坐标命中：双击「编辑」的第二击落在同一槽位换上来的「更新」提交按钮上。
		// jsdom 无坐标命中，按角色查询同一位置的新按钮等价模拟。
		fireEvent.click(screen.getByRole("button", { name: "编辑" }));
		const updateBtn = screen.getByRole("button", { name: "更新" }) as HTMLButtonElement;
		fireEvent.click(updateBtn);

		// zodResolver 校验异步完成，先冲刷微任务让提交流程跑完再断言。
		await act(async () => {
			await new Promise((resolve) => setTimeout(resolve, 20));
		});

		expect(mocks.updateMutate).not.toHaveBeenCalled();
		expect(mocks.toastSuccess).not.toHaveBeenCalled();
		expect(updateBtn.disabled).toBe(true);
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

describe("ProviderModelDetailDialog 模型级代理", () => {
	it("只读态展示网络代理：开启显示徽标与地址，关闭显示未开启", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel({ proxyEnabled: true, proxyAddr: "http://127.0.0.1:7890" })}
			/>,
		);

		expect(screen.getByText("使用网络代理")).toBeTruthy();
		expect(screen.getByText("已开启")).toBeTruthy();
		expect(screen.getByText("http://127.0.0.1:7890")).toBeTruthy();

		// 关闭代理的模型：只显示「未开启」，不显示地址。
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				model={makeModel({ proxyEnabled: false, proxyAddr: "" })}
			/>,
		);
		expect(screen.getAllByText("未开启").length).toBeGreaterThan(0);
	});

	it("模型级关闭但供应商开启时展示「继承供应商代理」，避免误判为直连", () => {
		render(
			<ProviderModelDetailDialog
				open
				onOpenChange={vi.fn()}
				providerId={7}
				providerName="OpenAI"
				providerProxyAddr="http://127.0.0.1:7891"
				model={makeModel({ proxyEnabled: false, proxyAddr: "" })}
			/>,
		);

		expect(screen.getAllByText("未开启").length).toBeGreaterThan(0);
		expect(screen.getByText("· 继承供应商代理")).toBeTruthy();
		// 供应商代理地址不直接展示给模型级（地址属于供应商层）。
		expect(screen.queryByText("http://127.0.0.1:7891")).toBeNull();
	});

	it("编辑态开启代理并填地址后提交，payload 携带代理字段", async () => {
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
		// 点击代理开关（打开），随后出现地址输入框。
		const proxySwitch = screen.getByRole("switch", { name: "使用网络代理" });
		fireEvent.click(proxySwitch);
		const addrInput = screen.getByPlaceholderText("http://127.0.0.1:7890");
		fireEvent.change(addrInput, { target: { value: "http://127.0.0.1:7890" } });
		fireEvent.click(screen.getByRole("button", { name: "更新" }));

		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.updateMutate.mock.calls[0])[0];
		expect(payload.proxyEnabled).toBe(true);
		expect(payload.proxyAddr).toBe("http://127.0.0.1:7890");
	});

	it("开启代理但地址为空时不提交（校验错误）", async () => {
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
		fireEvent.click(screen.getByRole("switch", { name: "使用网络代理" }));
		fireEvent.click(screen.getByRole("button", { name: "更新" }));

		await act(async () => {
			await new Promise((resolve) => setTimeout(resolve, 20));
		});
		expect(mocks.updateMutate).not.toHaveBeenCalled();
		expect(screen.getByText("开启网络代理时必须填写代理地址")).toBeTruthy();
	});
});
