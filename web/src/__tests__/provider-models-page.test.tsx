import type { ProviderModel } from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import ProviderModelsPage from "@/pages/provider-models";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		providers: undefined as Provider[] | undefined,
		models: undefined as ProviderModel[] | undefined,
		isLoading: false,
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
	ProviderModelDetailDialog: () => null,
}));

function makeProvider(id: number, name: string): Provider {
	return {
		id,
		name,
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
		createdAt: "",
		updatedAt: "",
	};
}

describe("ProviderModelsPage", () => {
	beforeEach(() => {
		mocks.providers = undefined;
		mocks.models = undefined;
		mocks.isLoading = false;
	});

	it("没有供应商时展示空态引导", () => {
		mocks.providers = [];
		render(
			<MemoryRouter>
				<ProviderModelsPage />
			</MemoryRouter>,
		);

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
		render(
			<MemoryRouter>
				<ProviderModelsPage />
			</MemoryRouter>,
		);

		expect(screen.getByRole("heading", { name: "OpenAI" })).toBeTruthy();
		expect(screen.getByRole("heading", { name: "DeepSeek" })).toBeTruthy();
		expect(screen.getAllByRole("button", { name: "添加" })).toHaveLength(2);

		expect(screen.getByRole("button", { name: /gpt-4o/ })).toBeTruthy();
		expect(screen.getByRole("button", { name: /o3/ })).toBeTruthy();
		expect(screen.getByRole("button", { name: /deepseek-chat/ })).toBeTruthy();
	});

	it("供应商暂无模型时显示占位提示", () => {
		mocks.providers = [makeProvider(1, "OpenAI")];
		mocks.models = [];
		render(
			<MemoryRouter>
				<ProviderModelsPage />
			</MemoryRouter>,
		);

		expect(screen.getByText(/暂无模型/)).toBeTruthy();
	});
});
