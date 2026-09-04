import type { ProviderModel } from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import type { VirtualModel, VirtualModelItem } from "@/hooks/use-virtual-models";
import VirtualModelsPage from "@/pages/virtual-models";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		virtualModels: undefined as VirtualModel[] | undefined,
		providers: undefined as Provider[] | undefined,
		providerModels: undefined as ProviderModel[] | undefined,
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
		editDialogOpen: false,
		deleteDialogOpen: false,
		detailDialogOpen: false,
		detailItem: null as VirtualModelItem | null,
		detailVirtualModel: null as VirtualModel | null,
		navigate: vi.fn(),
	};
});

vi.mock("react-router-dom", async () => {
	const actual = await vi.importActual<typeof import("react-router-dom")>("react-router-dom");
	return {
		...actual,
		useNavigate: () => mocks.navigate,
	};
});

vi.mock("@/hooks/use-virtual-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-virtual-models")>(
		"@/hooks/use-virtual-models",
	);
	return {
		...actual,
		useVirtualModels: () => ({
			data: mocks.virtualModels,
			isLoading: mocks.isLoading,
			isError: mocks.isError,
			refetch: mocks.refetch,
		}),
	};
});

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useProviders: () => ({
			data: mocks.providers,
			isLoading: false,
			isError: false,
			refetch: vi.fn(),
		}),
	};
});

vi.mock("@/hooks/use-provider-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-provider-models")>(
		"@/hooks/use-provider-models",
	);
	return {
		...actual,
		useProviderModels: () => ({
			data: mocks.providerModels,
			isLoading: false,
			isError: false,
			refetch: vi.fn(),
		}),
	};
});

vi.mock("@/components/virtual-models/VirtualModelEditDialog", () => ({
	VirtualModelEditDialog: (props: { open: boolean }) => {
		mocks.editDialogOpen = props.open;
		return null;
	},
}));

vi.mock("@/components/virtual-models/VirtualModelDeleteDialog", () => ({
	VirtualModelDeleteDialog: (props: { open: boolean }) => {
		mocks.deleteDialogOpen = props.open;
		return null;
	},
}));

vi.mock("@/components/virtual-models/VirtualModelItemDetailDialog", () => ({
	VirtualModelItemDetailDialog: (props: {
		open: boolean;
		item: VirtualModelItem | null;
		virtualModel: VirtualModel | null;
	}) => {
		mocks.detailDialogOpen = props.open;
		mocks.detailItem = props.item;
		mocks.detailVirtualModel = props.virtualModel;
		return null;
	},
}));

let nextItemId = 1;

function makeItem(overrides: Partial<VirtualModelItem> = {}): VirtualModelItem {
	return {
		virtualModelItemId: nextItemId++,
		modelId: 11,
		enable: true,
		providerId: 7,
		providerName: "OpenAI",
		providerEnable: true,
		billingMode: 0,
		providerModelId: "gpt-4o@openai",
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
		virtualModelId: 1,
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

function renderPage() {
	return render(
		<MemoryRouter>
			<VirtualModelsPage />
		</MemoryRouter>,
	);
}

describe("VirtualModelsPage", () => {
	beforeEach(() => {
		nextItemId = 1;
		mocks.virtualModels = undefined;
		mocks.providers = undefined;
		mocks.providerModels = undefined;
		mocks.isLoading = false;
		mocks.isError = false;
		mocks.editDialogOpen = false;
		mocks.deleteDialogOpen = false;
		mocks.detailDialogOpen = false;
		mocks.detailItem = null;
		mocks.detailVirtualModel = null;
	});

	it("加载中渲染骨架屏，不渲染内容", () => {
		mocks.isLoading = true;
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		expect(screen.queryByText(/暂无虚拟模型/)).toBeNull();
		expect(screen.queryByRole("button", { name: "添加虚拟模型" })).toBeNull();
	});

	it("加载失败展示错误态", () => {
		mocks.isError = true;
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		expect(screen.getByText(/无法获取虚拟模型数据/)).toBeTruthy();
	});

	it("默认没有虚拟模型：空态文案 + 添加按钮打开创建弹窗", () => {
		mocks.virtualModels = [];
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		expect(screen.getByText(/暂无虚拟模型/)).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: "添加虚拟模型" }));
		expect(mocks.editDialogOpen).toBe(true);
	});

	it("每个虚拟模型一个区块：名称 + 策略 + 平铺成员卡片（含停用标记）", () => {
		mocks.virtualModels = [
			makeVm({
				virtualModelId: 1,
				displayId: "gpt-4o",
				items: [
					makeItem(),
					makeItem({
						virtualModelItemId: 2,
						modelId: 12,
						providerModelId: "o3@openai",
						enable: false,
					}),
				],
			}),
			makeVm({ virtualModelId: 2, displayId: "claude-sonnet" }),
		];
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		expect(screen.getByRole("heading", { name: "gpt-4o" })).toBeTruthy();
		expect(screen.getByRole("heading", { name: "claude-sonnet" })).toBeTruthy();
		// 两个区块都是默认策略。
		expect(screen.getAllByText("订阅制优先").length).toBe(2);
		expect(screen.getAllByText("直接失败").length).toBe(2);
		expect(screen.getByText("gpt-4o@openai")).toBeTruthy();
		expect(screen.getAllByText("OpenAI").length).toBe(2);
		expect(screen.getByText("o3@openai")).toBeTruthy();
		expect(screen.getByText(/已停用/)).toBeTruthy();
		// 无成员的虚拟模型区块显示占位提示。
		expect(screen.getByText(/暂无成员模型/)).toBeTruthy();
	});

	it("区块标题与成员 ID 带方向键：标题链接到虚拟模型数据面板，成员 ID 编程跳转模型数据面板", () => {
		mocks.virtualModels = [
			makeVm({
				virtualModelId: 1,
				displayId: "gpt-4o",
				items: [makeItem({ virtualModelItemId: 1, providerModelId: "gpt-4o@openai" })],
			}),
		];
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		// 区块标题 displayId → 虚拟模型数据面板（真实链接）。
		const vmLink = screen.getByRole("link", { name: /gpt-4o/ }) as HTMLAnchorElement;
		expect(vmLink.getAttribute("href")).toBe("/virtual-models/1/overview");
		expect(vmLink.querySelector("svg")).toBeTruthy();

		// 成员 ID 区 → 编程跳转模型数据面板（卡片无 <a> 内嵌）。
		fireEvent.click(screen.getByText("gpt-4o@openai"));
		expect(mocks.navigate).toHaveBeenCalledWith("/models/7/gpt-4o%40openai/overview");

		// 点击成员卡片空白仍打开详情弹窗。
		fireEvent.click(screen.getByTestId("virtual-model-member-1"));
		expect(mocks.detailDialogOpen).toBe(true);
	});

	it("区块菜单：点「编辑」打开编辑弹窗", async () => {
		mocks.virtualModels = [makeVm()];
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		// Radix DropdownMenu 在 jsdom 下通过键盘事件打开。
		fireEvent.keyDown(screen.getByRole("button", { name: "更多操作：gpt-4o" }), {
			key: "Enter",
		});

		fireEvent.click(await screen.findByRole("menuitem", { name: "编辑" }));
		await waitFor(() => expect(mocks.editDialogOpen).toBe(true));
	});

	it("区块菜单：点「删除」打开删除确认弹窗", async () => {
		mocks.virtualModels = [makeVm()];
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		fireEvent.keyDown(screen.getByRole("button", { name: "更多操作：gpt-4o" }), {
			key: "Enter",
		});

		fireEvent.click(await screen.findByRole("menuitem", { name: "删除" }));
		await waitFor(() => expect(mocks.deleteDialogOpen).toBe(true));
	});

	it("点击成员卡片打开详情弹窗并传入目标成员；停用/随供应商禁用/虚拟模型停用均可点", () => {
		const enabled = makeItem({
			virtualModelItemId: 1,
			modelId: 11,
			providerModelId: "gpt-4o@openai",
			enable: true,
			providerEnable: true,
		});
		const disabledItem = makeItem({
			virtualModelItemId: 2,
			modelId: 12,
			providerModelId: "o3@openai",
			enable: false,
			providerEnable: true,
		});
		const disabledWithProvider = makeItem({
			virtualModelItemId: 3,
			modelId: 13,
			providerModelId: "o4@openai",
			enable: true,
			providerEnable: false,
		});
		mocks.virtualModels = [
			makeVm({
				virtualModelId: 1,
				displayId: "gpt-4o",
				enable: true,
				items: [enabled, disabledItem, disabledWithProvider],
			}),
			makeVm({
				virtualModelId: 2,
				displayId: "claude",
				enable: false,
				items: [makeItem({ virtualModelItemId: 4, providerModelId: "claude-sonnet@anthropic" })],
			}),
		];
		render(
			<MemoryRouter>
				<VirtualModelsPage />
			</MemoryRouter>,
		);

		// 正常成员。
		fireEvent.click(screen.getByRole("button", { name: /gpt-4o@openai/ }));
		expect(mocks.detailDialogOpen).toBe(true);
		expect(mocks.detailItem?.modelId).toBe(11);
		expect(mocks.detailVirtualModel?.virtualModelId).toBe(1);

		// 关闭复位后再点已停用成员。
		mocks.detailDialogOpen = false;
		fireEvent.click(screen.getByRole("button", { name: /o3@openai/ }));
		expect(mocks.detailDialogOpen).toBe(true);
		expect(mocks.detailItem?.modelId).toBe(12);

		// 随供应商禁用成员。
		mocks.detailDialogOpen = false;
		fireEvent.click(screen.getByRole("button", { name: /o4@openai/ }));
		expect(mocks.detailDialogOpen).toBe(true);
		expect(mocks.detailItem?.modelId).toBe(13);

		// 虚拟模型本身停用区块中的成员仍可点击打开详情。
		mocks.detailDialogOpen = false;
		fireEvent.click(screen.getByRole("button", { name: /claude-sonnet@anthropic/ }));
		expect(mocks.detailDialogOpen).toBe(true);
		expect(mocks.detailVirtualModel?.virtualModelId).toBe(2);
	});

	it("按成员 providerModelId 搜索，结果按所属虚拟模型分组（含停用虚拟模型与停用成员）", () => {
		mocks.virtualModels = [
			makeVm({
				virtualModelId: 1,
				displayId: "gpt-4o",
				items: [makeItem({ modelId: 11, providerModelId: "gpt-deepseek" })],
			}),
			makeVm({
				virtualModelId: 2,
				displayId: "deepseek",
				enable: false,
				items: [
					makeItem({ modelId: 12, providerModelId: "deepseek-chat" }),
					makeItem({
						modelId: 13,
						providerModelId: "deepseek-reasoner",
						enable: false,
						providerEnable: false,
					}),
				],
			}),
			makeVm({
				virtualModelId: 3,
				displayId: "claude",
				items: [makeItem({ modelId: 14, providerModelId: "claude-sonnet@anthropic" })],
			}),
		];
		renderPage();

		fireEvent.change(screen.getByRole("searchbox", { name: "搜索虚拟模型成员" }), {
			target: { value: "DeepSeek" },
		});
		// 匹配不区分大小写：混合大小写关键词命中小写成员 ID。
		const group1 = screen.getByTestId("virtual-model-search-group-1");
		const group2 = screen.getByTestId("virtual-model-search-group-2");
		expect(group1).toHaveTextContent("gpt-4o");
		expect(group2).toHaveTextContent("deepseek");
		// 未命中（无 deepseek 成员）的虚拟模型不出现。
		expect(screen.queryByTestId("virtual-model-search-group-3")).toBeNull();
		// 组内仅列命中成员：停用的虚拟模型与随供应商禁用+已停用的成员也可见。
		const gptDeepseek = within(group1).getByRole("button", { name: /gpt-deepseek/ });
		expect(gptDeepseek).toHaveTextContent("OpenAI");
		expect(within(group2).getByRole("button", { name: /deepseek-chat/ })).toBeTruthy();
		expect(within(group2).getByRole("button", { name: /deepseek-reasoner/ })).toBeTruthy();
	});

	it("点击搜索结果打开成员详情弹窗并保留搜索；无命中展示提示", () => {
		mocks.virtualModels = [
			makeVm({
				virtualModelId: 1,
				displayId: "gpt-4o",
				items: [
					makeItem({ modelId: 11, providerModelId: "gpt-4o@openai" }),
					makeItem({ modelId: 12, providerModelId: "o3@openai" }),
				],
			}),
		];
		renderPage();

		const searchbox = screen.getByRole("searchbox", { name: "搜索虚拟模型成员" });
		fireEvent.change(searchbox, { target: { value: "o3" } });
		const group = screen.getByTestId("virtual-model-search-group-1");
		fireEvent.click(within(group).getByRole("button", { name: /o3@openai/ }));
		// 详情弹窗打开并传入命中成员与其所属虚拟模型。
		expect(mocks.detailDialogOpen).toBe(true);
		expect(mocks.detailItem?.modelId).toBe(12);
		expect(mocks.detailVirtualModel?.virtualModelId).toBe(1);

		// 详情弹窗关闭（mock 记录 open=false）后，关键词与结果面板保留。
		mocks.detailDialogOpen = false;
		expect(searchbox).toHaveValue("o3");
		expect(screen.getByTestId("virtual-model-search-group-1")).toBeTruthy();

		// 无命中时展示无结果文案。
		fireEvent.change(searchbox, { target: { value: "不存在的模型" } });
		expect(screen.queryByTestId("virtual-model-search-group-1")).toBeNull();
		expect(screen.getByText(/未找到匹配/)).toBeTruthy();
	});

	it("点击搜索结果区域外收起结果面板", () => {
		mocks.virtualModels = [
			makeVm({
				virtualModelId: 1,
				displayId: "gpt-4o",
				items: [makeItem({ modelId: 11, providerModelId: "gpt-4o@openai" })],
			}),
		];
		renderPage();

		fireEvent.change(screen.getByRole("searchbox", { name: "搜索虚拟模型成员" }), {
			target: { value: "gpt" },
		});
		expect(screen.getByTestId("virtual-model-search-results")).toBeTruthy();

		fireEvent.pointerDown(screen.getByRole("heading", { name: "虚拟模型" }));
		expect(screen.queryByTestId("virtual-model-search-results")).toBeNull();
	});
});
