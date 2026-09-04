import { VirtualModelDeleteDialog } from "@/components/virtual-models/VirtualModelDeleteDialog";
import { VirtualModelEditDialog } from "@/components/virtual-models/VirtualModelEditDialog";
import { VirtualModelItemDetailDialog } from "@/components/virtual-models/VirtualModelItemDetailDialog";
import type { ProviderModel } from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import type { VirtualModel, VirtualModelItem } from "@/hooks/use-virtual-models";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		createMutate: vi.fn(),
		updateMutate: vi.fn(),
		deleteMutate: vi.fn(),
	};
});

vi.mock("@/hooks/use-virtual-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-virtual-models")>(
		"@/hooks/use-virtual-models",
	);
	return {
		...actual,
		useCreateVirtualModel: () => ({ mutate: mocks.createMutate, isPending: false }),
		useUpdateVirtualModel: () => ({ mutate: mocks.updateMutate, isPending: false }),
		useDeleteVirtualModel: () => ({ mutate: mocks.deleteMutate, isPending: false }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: vi.fn(),
		toastError: vi.fn(),
	}),
}));

const providers: Provider[] = [
	{
		id: 7,
		name: "OpenAI",
		enable: true,
		baseUrl: "https://api.example.com/v1",
		apiKeyMasked: "sk-****test",
		protocolType: 0,
		billingMode: 0,
		customHeader: "{}",
		extra: "{}",
		proxyEnabled: false,
		proxyAddr: "",
		createdAt: "",
		updatedAt: "",
	},
	{
		id: 8,
		name: "DeepSeek",
		enable: true,
		baseUrl: "https://api.deepseek.com/v1",
		apiKeyMasked: "sk-****test",
		protocolType: 0,
		billingMode: 0,
		customHeader: "{}",
		extra: "{}",
		proxyEnabled: false,
		proxyAddr: "",
		createdAt: "",
		updatedAt: "",
	},
];

function makeModel(overrides: Partial<ProviderModel> = {}): ProviderModel {
	return {
		modelId: 11,
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
		createdAt: "",
		updatedAt: "",
		...overrides,
	};
}

function makeItem(overrides: Partial<VirtualModelItem> = {}): VirtualModelItem {
	return {
		virtualModelItemId: 1,
		modelId: 11,
		enable: true,
		providerId: 7,
		providerName: "OpenAI",
		providerEnable: true,
		billingMode: 0,
		providerModelId: "gpt-4o",
		contextLength: 128000,
		maxOutputTokens: 4096,
		reasoning: true,
		toolUse: false,
		imageUnderstand: false,
		videoUnderstand: false,
		...overrides,
	};
}

function makeVm(overrides: Partial<VirtualModel> = {}): VirtualModel {
	return {
		virtualModelId: 3,
		displayId: "gpt-4o",
		enable: true,
		loadBalancingStrategy: 0,
		fallbackStrategy: 0,
		items: [],
		createdAt: "",
		updatedAt: "",
		...overrides,
	};
}

/** 收窄可能为 undefined 的元素/数组项（tsc 开启 noUncheckedIndexedAccess）。 */
function required<T>(value: T | undefined | null): T {
	if (value === undefined || value === null) {
		throw new Error("required element/value not found");
	}
	return value;
}

/** 展开某供应商分组的候选区。 */
function openAddGroup(providerName: string) {
	fireEvent.click(screen.getByRole("button", { name: `在 ${providerName} 中添加模型` }));
}

beforeEach(() => {
	vi.clearAllMocks();
});

describe("VirtualModelEditDialog 创建模式", () => {
	it("默认值：启用开、订阅制优先、直接失败；暂存为空时保存禁用", () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={null}
				providers={providers}
				providerModels={[makeModel()]}
				mappedModelIds={new Set()}
			/>,
		);

		const saveBtn = screen.getByRole("button", { name: "创建" }) as HTMLButtonElement;
		expect(saveBtn.disabled).toBe(true);
		expect(screen.getByText("至少保留一个成员模型")).toBeTruthy();
	});

	it("展开供应商分组 → 点「添加」加入暂存 → 填模型 ID → 创建 payload 正确", async () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={null}
				providers={providers}
				providerModels={[
					makeModel({ modelId: 11, providerId: 7, providerModelId: "gpt-4o" }),
					makeModel({ modelId: 12, providerId: 7, providerModelId: "o3" }),
					makeModel({ modelId: 21, providerId: 8, providerModelId: "deepseek-chat" }),
				]}
				mappedModelIds={new Set([12])}
			/>,
		);

		// 已被其他虚拟模型占用的 o3 不出现在候选区。
		openAddGroup("OpenAI");
		expect(screen.getByRole("button", { name: "添加 gpt-4o" })).toBeTruthy();
		expect(screen.queryByRole("button", { name: "添加 o3" })).toBeNull();

		fireEvent.click(screen.getByRole("button", { name: "添加 gpt-4o" }));
		expect(screen.getByText("已选 1 个成员模型")).toBeTruthy();

		fireEvent.change(screen.getByPlaceholderText("如 gpt-4o"), {
			target: { value: "gpt-4o" },
		});
		fireEvent.click(screen.getByRole("button", { name: "创建" }));

		await waitFor(() => expect(mocks.createMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.createMutate.mock.calls[0])[0];
		expect(payload.displayId).toBe("gpt-4o");
		expect(payload.enable).toBe(true);
		expect(payload.loadBalancingStrategy).toBe(0);
		expect(payload.fallbackStrategy).toBe(0);
		expect(payload.items).toEqual([{ modelId: 11, enable: true }]);
	});

	it("从两个供应商分别添加成员，payload 汇总两组", async () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={null}
				providers={providers}
				providerModels={[
					makeModel({ modelId: 11, providerId: 7, providerModelId: "gpt-4o" }),
					makeModel({ modelId: 21, providerId: 8, providerModelId: "deepseek-chat" }),
				]}
				mappedModelIds={new Set()}
			/>,
		);

		openAddGroup("OpenAI");
		fireEvent.click(screen.getByRole("button", { name: "添加 gpt-4o" }));
		openAddGroup("DeepSeek");
		fireEvent.click(screen.getByRole("button", { name: "添加 deepseek-chat" }));

		expect(screen.getByText("OpenAI")).toBeTruthy();
		expect(screen.getByText("DeepSeek")).toBeTruthy();
		expect(screen.getByText("已选 2 个成员模型")).toBeTruthy();

		fireEvent.change(screen.getByPlaceholderText("如 gpt-4o"), { target: { value: "gpt-4o" } });
		fireEvent.click(screen.getByRole("button", { name: "创建" }));

		await waitFor(() => expect(mocks.createMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.createMutate.mock.calls[0])[0];
		expect(payload.items).toEqual([
			{ modelId: 11, enable: true },
			{ modelId: 21, enable: true },
		]);
	});

	it("模型 ID 为空时不提交", () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={null}
				providers={providers}
				providerModels={[makeModel()]}
				mappedModelIds={new Set()}
			/>,
		);

		openAddGroup("OpenAI");
		fireEvent.click(screen.getByRole("button", { name: "添加 gpt-4o" }));
		fireEvent.click(screen.getByRole("button", { name: "创建" }));

		expect(mocks.createMutate).not.toHaveBeenCalled();
	});
});

describe("VirtualModelEditDialog 编辑模式", () => {
	it("回填 displayId/启停/策略与成员；暂存修改后保存 payload 汇总生效", async () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVm({
					virtualModelId: 3,
					displayId: "gpt-4o",
					enable: false,
					loadBalancingStrategy: 2,
					fallbackStrategy: 1,
					items: [
						makeItem({ modelId: 11, enable: false }),
						makeItem({
							virtualModelItemId: 2,
							modelId: 12,
							providerModelId: "o3",
							enable: true,
						}),
					],
				})}
				providers={providers}
				providerModels={[
					makeModel({ modelId: 11, providerModelId: "gpt-4o" }),
					makeModel({ modelId: 12, providerModelId: "o3" }),
					makeModel({ modelId: 13, providerModelId: "o3-mini" }),
				]}
				mappedModelIds={new Set()}
			/>,
		);

		const displayInput = screen.getByPlaceholderText("如 gpt-4o") as HTMLInputElement;
		expect(displayInput.value).toBe("gpt-4o");

		// 回填的成员：o3 启用、gpt-4o 停用（带「已停用」标记）。
		expect(
			(screen.getByRole("switch", { name: "启停 o3" }) as HTMLButtonElement).getAttribute(
				"aria-checked",
			),
		).toBe("true");
		expect(
			(screen.getByRole("switch", { name: "启停 gpt-4o" }) as HTMLButtonElement).getAttribute(
				"aria-checked",
			),
		).toBe("false");
		expect(screen.getByText(/已停用/)).toBeTruthy();

		// 暂存操作：启停 gpt-4o、移除 o3、添加 o3-mini。
		fireEvent.click(screen.getByRole("switch", { name: "启停 gpt-4o" }));
		fireEvent.click(screen.getByRole("button", { name: "移除 o3" }));
		openAddGroup("OpenAI");
		fireEvent.click(screen.getByRole("button", { name: "添加 o3-mini" }));

		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.updateMutate.mock.calls[0])[0];
		expect(payload.id).toBe(3);
		expect(payload.displayId).toBe("gpt-4o");
		expect(payload.enable).toBe(false);
		expect(payload.loadBalancingStrategy).toBe(2);
		expect(payload.fallbackStrategy).toBe(1);
		expect(payload.items).toEqual([
			{ modelId: 11, enable: true },
			{ modelId: 13, enable: true },
		]);
	});

	it("已停用成员在保存时保留 enable=false", async () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVm({
					items: [makeItem({ modelId: 11, enable: false })],
				})}
				providers={providers}
				providerModels={[makeModel()]}
				mappedModelIds={new Set()}
			/>,
		);

		// 不做任何修改，直接保存 → enable 原样保留。
		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.updateMutate.mock.calls[0])[0];
		expect(payload.items).toEqual([{ modelId: 11, enable: false }]);
	});

	it("移除全部成员后保存禁用", () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVm({
					items: [makeItem({ modelId: 11 })],
				})}
				providers={providers}
				providerModels={[makeModel()]}
				mappedModelIds={new Set()}
			/>,
		);

		fireEvent.click(screen.getByRole("button", { name: "移除 gpt-4o" }));
		const saveBtn = screen.getByRole("button", { name: "保存" }) as HTMLButtonElement;
		expect(saveBtn.disabled).toBe(true);
	});
});

describe("VirtualModelItemDetailDialog", () => {
	it("只读展示条目详情与状态标记；无编辑/删除/测试按钮", () => {
		render(
			<VirtualModelItemDetailDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVm({
					displayId: "gpt-4o",
					items: [
						makeItem({
							enable: false,
							providerEnable: false,
							contextLength: 128000,
							maxOutputTokens: 4096,
						}),
					],
				})}
				item={makeItem({
					enable: false,
					providerEnable: false,
					contextLength: 128000,
					maxOutputTokens: 4096,
				})}
			/>,
		);

		// 标题为远端模型 ID，描述含所属供应商。
		expect(screen.getByRole("heading", { name: "gpt-4o" })).toBeTruthy();
		expect(screen.getByText(/OpenAI/)).toBeTruthy();
		// 上下文/最大输出用全量数字。
		expect(screen.getByText("128,000")).toBeTruthy();
		expect(screen.getByText("4,096")).toBeTruthy();
		// 能力：推理已支持、工具调用不支持。
		expect(screen.getByText(/推理/)).toBeTruthy();
		expect(screen.getByText(/工具调用/)).toBeTruthy();
		// 状态标记。
		expect(screen.getByText(/已停用/)).toBeTruthy();
		expect(screen.getByText(/随供应商禁用/)).toBeTruthy();
		// 只读：无编辑/删除/测试按钮。
		expect(screen.queryByRole("button", { name: /编辑/ })).toBeNull();
		expect(screen.queryByRole("button", { name: /删除/ })).toBeNull();
		expect(screen.queryByRole("button", { name: /测试/ })).toBeNull();
	});

	it("拨动启停开关：提交翻转后的完整成员集合，其余成员不变", async () => {
		render(
			<VirtualModelItemDetailDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVm({
					virtualModelId: 3,
					displayId: "gpt-4o",
					loadBalancingStrategy: 0,
					fallbackStrategy: 0,
					items: [
						makeItem({ virtualModelItemId: 1, modelId: 11, providerModelId: "gpt-4o" }),
						makeItem({
							virtualModelItemId: 2,
							modelId: 12,
							providerModelId: "o3",
							enable: false,
						}),
					],
				})}
				item={makeItem({ virtualModelItemId: 1, modelId: 11, providerModelId: "gpt-4o" })}
			/>,
		);

		const toggle = screen.getByRole("switch", { name: /在虚拟模型中启用/ }) as HTMLButtonElement;
		expect(toggle.getAttribute("aria-checked")).toBe("true");

		fireEvent.click(toggle);
		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.updateMutate.mock.calls[0])[0];
		expect(payload.id).toBe(3);
		// 只翻转目标成员，其余成员原样保留。
		expect(payload.items).toEqual([
			{ modelId: 11, enable: false },
			{ modelId: 12, enable: false },
		]);
	});

	it("供应商停用时开关仍可操作：拨动仍提交翻转", async () => {
		render(
			<VirtualModelItemDetailDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVm({
					virtualModelId: 3,
					displayId: "gpt-4o",
					items: [makeItem({ virtualModelItemId: 1, modelId: 11, providerModelId: "gpt-4o" })],
				})}
				item={makeItem({ providerEnable: false })}
			/>,
		);

		const toggle = screen.getByRole("switch", { name: /在虚拟模型中启用/ }) as HTMLButtonElement;
		expect(toggle.getAttribute("aria-checked")).toBe("true");
		fireEvent.click(toggle);
		await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledTimes(1));
		const payload = required(mocks.updateMutate.mock.calls[0])[0];
		expect(payload.items).toEqual([{ modelId: 11, enable: false }]);
	});

	it("开关成功后关闭弹窗（避免快照陈旧导致二次翻转错位）", async () => {
		const onOpenChange = vi.fn();
		mocks.updateMutate.mockImplementation((_payload, options) => {
			required(options).onSuccess();
		});
		render(
			<VirtualModelItemDetailDialog
				open
				onOpenChange={onOpenChange}
				virtualModel={makeVm({
					virtualModelId: 3,
					displayId: "gpt-4o",
					items: [makeItem({ virtualModelItemId: 1, modelId: 11, providerModelId: "gpt-4o" })],
				})}
				item={makeItem({ virtualModelItemId: 1, modelId: 11, providerModelId: "gpt-4o" })}
			/>,
		);

		fireEvent.click(screen.getByRole("switch", { name: /在虚拟模型中启用/ }));
		await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
	});
});

describe("VirtualModelDeleteDialog", () => {
	it("二次确认后调用删除接口", async () => {
		render(<VirtualModelDeleteDialog open onOpenChange={vi.fn()} virtualModel={makeVm()} />);

		expect(screen.getByText(/确定要删除虚拟模型/)).toBeTruthy();
		const dialog = screen.getByRole("alertdialog");
		fireEvent.click(within(dialog).getByRole("button", { name: "删除" }));
		await waitFor(() => expect(mocks.deleteMutate).toHaveBeenCalledTimes(1));
		expect(required(mocks.deleteMutate.mock.calls[0])[0]).toBe(3);
	});
});
