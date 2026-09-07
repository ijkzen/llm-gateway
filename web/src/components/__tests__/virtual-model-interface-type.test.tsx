import { VirtualModelEditDialog } from "@/components/virtual-models/VirtualModelEditDialog";
import { acceptsProtocol, effectiveProtocol } from "@/components/virtual-models/draft-members";
import type { ProviderModel } from "@/hooks/use-provider-models";
import type { Provider } from "@/hooks/use-providers";
import type { VirtualModel } from "@/hooks/use-virtual-models";
import { INTERFACE_FULL_COMPATIBLE } from "@/lib/constants";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	createMutate: vi.fn(),
	updateMutate: vi.fn(),
	toastSuccess: vi.fn(),
	toastError: vi.fn(),
}));

vi.mock("@/hooks/use-virtual-models", async () => {
	const actual = await vi.importActual<typeof import("@/hooks/use-virtual-models")>(
		"@/hooks/use-virtual-models",
	);
	return {
		...actual,
		useCreateVirtualModel: () => ({ mutate: mocks.createMutate, isPending: false }),
		useUpdateVirtualModel: () => ({ mutate: mocks.updateMutate, isPending: false }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: mocks.toastSuccess,
		toastError: mocks.toastError,
	}),
}));

function makeProvider(overrides: Partial<Provider> = {}): Provider {
	return {
		id: 1,
		name: "OpenAI 供应商",
		enable: true,
		baseUrl: "https://api.example.com/v1",
		apiKeyMasked: "sk-****test",
		protocolType: 0,
		billingMode: 0,
		customHeader: "{}",
		extra: "{}",
		proxyEnabled: false,
		proxyAddr: "",
		createdAt: "2026-09-01T00:00:00Z",
		updatedAt: "2026-09-01T00:00:00Z",
		...overrides,
	};
}

function makeModel(overrides: Partial<ProviderModel> = {}): ProviderModel {
	return {
		modelId: 11,
		providerId: 1,
		providerName: "OpenAI",
		providerModelId: "gpt-x",
		contextLength: 128000,
		maxOutputTokens: 4096,
		reasoning: false,
		toolUse: true,
		imageUnderstand: false,
		videoUnderstand: false,
		protocolType: null,
		proxyEnabled: false,
		proxyAddr: "",
		createdAt: "2026-09-01T00:00:00Z",
		updatedAt: "2026-09-01T00:00:00Z",
		...overrides,
	};
}

function makeVirtualModel(overrides: Partial<VirtualModel> = {}): VirtualModel {
	return {
		virtualModelId: 1,
		displayId: "vm-a",
		enable: true,
		loadBalancingStrategy: 0,
		fallbackStrategy: 0,
		interfaceType: 4,
		items: [],
		createdAt: "2026-09-01T00:00:00Z",
		updatedAt: "2026-09-01T00:00:00Z",
		...overrides,
	};
}

beforeEach(() => {
	vi.clearAllMocks();
});

describe("draft-members：生效协议与接口类型匹配", () => {
	it("生效协议 = 模型级覆盖优先，否则供应商协议", () => {
		expect(effectiveProtocol(makeModel({ protocolType: 2 }), 0)).toBe(2);
		expect(effectiveProtocol(makeModel({ protocolType: null }), 3)).toBe(3);
		expect(effectiveProtocol(makeModel({ protocolType: null }), undefined)).toBe(0);
	});

	it("Full Compatible 接受全部协议，受限类型只接受本协议", () => {
		expect(acceptsProtocol(4, 0, INTERFACE_FULL_COMPATIBLE)).toBe(true);
		expect(acceptsProtocol(4, 3, INTERFACE_FULL_COMPATIBLE)).toBe(true);
		expect(acceptsProtocol(0, 0, INTERFACE_FULL_COMPATIBLE)).toBe(true);
		expect(acceptsProtocol(0, 2, INTERFACE_FULL_COMPATIBLE)).toBe(false);
		expect(acceptsProtocol(2, 2, INTERFACE_FULL_COMPATIBLE)).toBe(true);
		expect(acceptsProtocol(1, 2, INTERFACE_FULL_COMPATIBLE)).toBe(false);
	});
});

describe("VirtualModelEditDialog：接口类型", () => {
	it("创建模式默认 OpenAI Compatible，弹窗渲染候选过滤字段", () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={null}
				providers={[makeProvider()]}
				providerModels={[makeModel()]}
				mappedModelIds={new Set()}
			/>,
		);
		expect(screen.getAllByText("OpenAI Compatible").length).toBeGreaterThan(0);
	});

	it("编辑模式回填虚拟模型自身接口类型", () => {
		render(
			<VirtualModelEditDialog
				open
				onOpenChange={vi.fn()}
				virtualModel={makeVirtualModel({ interfaceType: 2 })}
				providers={[makeProvider()]}
				providerModels={[makeModel()]}
				mappedModelIds={new Set()}
			/>,
		);
		expect(screen.getAllByText("Messages（Anthropic）").length).toBeGreaterThan(0);
	});
});
