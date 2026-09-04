import type { ProviderModel } from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import ProviderModelsPage from "@/pages/provider-models";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		providers: undefined as Provider[] | undefined,
		models: undefined as ProviderModel[] | undefined,
		isLoading: false,
		updateProvider: vi.fn(),
		updateError: null as Error | null,
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

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useProviders: () => ({
			data: mocks.providers,
			isLoading: mocks.isLoading,
			isError: false,
			refetch: vi.fn(),
		}),
		useUpdateProvider: () => ({
			mutate: (
				payload: unknown,
				options: { onSuccess?: () => void; onError?: (error: Error) => void },
			) => {
				mocks.updateProvider(payload);
				if (mocks.updateError) {
					options.onError?.(mocks.updateError);
					return;
				}
				options.onSuccess?.();
			},
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
			data: mocks.models,
			isLoading: mocks.isLoading,
			isError: false,
			refetch: vi.fn(),
		}),
	};
});

vi.mock("@/components/provider-models/AddProviderModelsDialog", () => ({
	AddProviderModelsDialog: () => null,
}));

vi.mock("@/components/provider-models/ProviderModelDetailDialog", () => ({
	ProviderModelDetailDialog: ({ open, model }: { open: boolean; model: ProviderModel | null }) =>
		open ? <dialog open>{model?.providerModelId}</dialog> : null,
}));

function makeProvider(id: number, name: string, enable = true): Provider {
	return {
		id,
		name,
		enable,
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
	};
}

function makeModel(providerId: number, modelId: number, id: string): ProviderModel {
	return {
		modelId,
		providerId,
		providerModelId: id,
		contextLength: 128000,
		maxOutputTokens: 4096,
		reasoning: true,
		toolUse: false,
		imageUnderstand: false,
		videoUnderstand: false,
		proxyEnabled: false,
		proxyAddr: "",
		protocolType: null,
		createdAt: "",
		updatedAt: "",
	};
}

function renderPage() {
	return render(
		<MemoryRouter>
			<ProviderModelsPage />
		</MemoryRouter>,
	);
}

describe("ProviderModelsPage", () => {
	beforeEach(() => {
		mocks.providers = undefined;
		mocks.models = undefined;
		mocks.isLoading = false;
		mocks.updateProvider.mockReset();
		mocks.updateError = null;
	});

	it("没有供应商时展示空态引导", () => {
		mocks.providers = [];
		renderPage();

		expect(screen.getByText(/还没有供应商/)).toBeTruthy();
		expect(screen.getByRole("link", { name: "去创建供应商" })).toBeTruthy();
	});

	it("每个供应商一个区块：名称与添加按钮一左一右，卡片按供应商分组", () => {
		mocks.providers = [makeProvider(1, "OpenAI"), makeProvider(2, "DeepSeek")];
		mocks.models = [
			makeModel(1, 11, "gpt-4o"),
			makeModel(1, 12, "o3"),
			makeModel(2, 21, "deepseek-chat"),
		];
		renderPage();

		expect(screen.getByRole("heading", { name: /OpenAI/ })).toBeTruthy();
		expect(screen.getByRole("heading", { name: /DeepSeek/ })).toBeTruthy();
		expect(screen.getAllByRole("button", { name: "添加" })).toHaveLength(2);

		expect(screen.getByRole("button", { name: /gpt-4o/ })).toBeTruthy();
		expect(screen.getByRole("button", { name: /o3/ })).toBeTruthy();
		expect(screen.getByRole("button", { name: /deepseek-chat/ })).toBeTruthy();
	});

	it("供应商名称链接到数据面板；模型名称+方向键触发编程跳转；点卡片空白打开详情", () => {
		mocks.providers = [makeProvider(1, "OpenAI")];
		mocks.models = [makeModel(1, 11, "gpt-4o")];
		renderPage();
		mocks.navigate.mockClear();

		// 供应商区块标题仍是真实链接。
		const providerLink = screen.getByRole("link", { name: /OpenAI/ }) as HTMLAnchorElement;
		expect(providerLink.getAttribute("href")).toBe("/providers/1/overview");
		expect(providerLink.querySelector("svg")).toBeTruthy();

		// 模型名称 + 箭头：点击触发编程跳转到模型数据面板（卡片内无 <a>）。
		fireEvent.click(screen.getByText("gpt-4o"));
		expect(mocks.navigate).toHaveBeenCalledWith("/models/1/gpt-4o/overview");

		// 点击卡片空白区域（button 本体）打开详情弹窗。
		fireEvent.click(screen.getByTestId("provider-model-card-11"));
		expect(screen.getByRole("dialog")).toHaveTextContent("gpt-4o");
	});

	it("按供应商模型 ID 搜索、按供应商分组，点击结果打开详情并保留结果", () => {
		mocks.providers = [makeProvider(1, "OpenAI"), makeProvider(2, "DeepSeek", false)];
		mocks.models = [
			makeModel(1, 11, "gpt-4o"),
			makeModel(1, 12, "gpt-4.1"),
			makeModel(2, 21, "gpt-deepseek"),
		];
		renderPage();

		fireEvent.change(screen.getByRole("searchbox", { name: "搜索供应商模型" }), {
			target: { value: "gpt" },
		});
		expect(screen.getByTestId("provider-model-search-group-1")).toHaveTextContent("OpenAI");
		expect(screen.getByTestId("provider-model-search-group-2")).toHaveTextContent("DeepSeek");
		expect(screen.getByRole("button", { name: "gpt-deepseek" })).toBeTruthy();

		fireEvent.click(screen.getByRole("button", { name: "gpt-deepseek" }));
		expect(screen.getByRole("dialog")).toHaveTextContent("gpt-deepseek");
		expect(screen.getByRole("searchbox", { name: "搜索供应商模型" })).toHaveValue("gpt");
	});

	it("点击搜索结果外部会收起结果面板", () => {
		mocks.providers = [makeProvider(1, "OpenAI")];
		mocks.models = [makeModel(1, 11, "gpt-4o")];
		renderPage();

		fireEvent.change(screen.getByRole("searchbox", { name: "搜索供应商模型" }), {
			target: { value: "gpt" },
		});
		expect(screen.getByRole("button", { name: "gpt-4o" })).toBeTruthy();

		fireEvent.pointerDown(screen.getByRole("heading", { name: "供应商模型" }));
		expect(screen.queryByRole("button", { name: "gpt-4o" })).toBeNull();
	});

	it("供应商开关立即更新，失败时回滚", () => {
		mocks.providers = [makeProvider(1, "OpenAI")];
		mocks.models = [];
		renderPage();

		const toggle = screen.getByRole("switch", { name: "启用 OpenAI" });
		expect(toggle).toHaveAttribute("data-state", "checked");
		fireEvent.click(toggle);
		expect(mocks.updateProvider).toHaveBeenLastCalledWith({ id: 1, enable: false });
		expect(toggle).toHaveAttribute("data-state", "unchecked");

		mocks.updateError = new Error("更新失败");
		fireEvent.click(toggle);
		expect(mocks.updateProvider).toHaveBeenLastCalledWith({ id: 1, enable: true });
		expect(toggle).toHaveAttribute("data-state", "checked");
	});

	it("供应商暂无模型时显示占位提示", () => {
		mocks.providers = [makeProvider(1, "OpenAI")];
		mocks.models = [];
		renderPage();

		expect(screen.getByText(/暂无模型/)).toBeTruthy();
	});
});
