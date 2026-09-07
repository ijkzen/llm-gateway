import { type BreadcrumbRoute, Breadcrumbs, parseBreadcrumbRoute } from "@/components/breadcrumbs";
import type { ProviderModel } from "@/hooks/use-provider-models";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	pathname: "/providers/3/overview",
	detail: undefined as ProviderModel | undefined,
}));

vi.mock("react-router-dom", () => ({
	useLocation: () => ({ pathname: mocks.pathname }),
	Link: ({ to, children }: { to: string; children: React.ReactNode }) => (
		<a href={to}>{children}</a>
	),
}));

vi.mock("@/hooks/use-provider-models", () => ({
	useProviderModelDetail: () => ({
		data: mocks.detail,
		isError: false,
		refetch: vi.fn(),
	}),
}));

function makeDetail(overrides: Partial<ProviderModel> = {}): ProviderModel {
	return {
		modelId: 11,
		providerId: 3,
		providerName: "火山方舟",
		providerModelId: "deepseek-v3",
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
		...overrides,
	};
}

describe("parseBreadcrumbRoute", () => {
	it.each<[string, BreadcrumbRoute | null]>([
		["/providers/3/overview", { kind: "provider", providerId: 3 }],
		["/virtual-models/5/overview", { kind: "virtualModel", virtualModelId: 5 }],
		["/api-keys/7/overview", { kind: "apiKey", apiKeyId: 7 }],
		["/models/11/overview", { kind: "model", modelId: 11 }],
		["/", null],
		["/providers", null],
		["/providers/3", null],
		["/request-logs", null],
		["/providers/abc/overview", null],
	])("解析 %s", (pathname, expected) => {
		expect(parseBreadcrumbRoute(pathname)).toEqual(expected);
	});
});

describe("Breadcrumbs", () => {
	beforeEach(() => {
		mocks.detail = undefined;
	});

	it("非数据面板详情页不渲染", () => {
		mocks.pathname = "/";
		const { container } = render(<Breadcrumbs />);
		expect(container.firstChild).toBeNull();
	});

	it("供应商面板 = 数据面板 › 供应商（链接到供应商列表）", () => {
		mocks.pathname = "/providers/3/overview";
		render(<Breadcrumbs />);
		const links = screen.getAllByRole("link");
		expect(links[0]).toHaveTextContent("数据面板");
		expect(links[0]).toHaveAttribute("href", "/");
		expect(links[1]).toHaveTextContent("供应商");
		expect(links[1]).toHaveAttribute("href", "/providers");
	});

	it("虚拟模型面板 = 数据面板 › 虚拟模型", () => {
		mocks.pathname = "/virtual-models/5/overview";
		render(<Breadcrumbs />);
		const links = screen.getAllByRole("link");
		expect(links).toHaveLength(2);
		expect(links[1]).toHaveTextContent("虚拟模型");
		expect(links[1]).toHaveAttribute("href", "/virtual-models");
	});

	it("API Key 面板 = 数据面板 › API Keys", () => {
		mocks.pathname = "/api-keys/7/overview";
		render(<Breadcrumbs />);
		const links = screen.getAllByRole("link");
		expect(links).toHaveLength(2);
		expect(links[1]).toHaveTextContent("API Keys");
		expect(links[1]).toHaveAttribute("href", "/api-keys");
	});

	it("模型面板 = 数据面板 › 供应商 › {所属供应商名}（供应商名链接到其数据面板）", () => {
		mocks.pathname = "/models/11/overview";
		mocks.detail = makeDetail();
		render(<Breadcrumbs />);
		const links = screen.getAllByRole("link");
		expect(links).toHaveLength(3);
		expect(links[1]).toHaveTextContent("供应商");
		expect(links[1]).toHaveAttribute("href", "/providers");
		expect(links[2]).toHaveTextContent("火山方舟");
		expect(links[2]).toHaveAttribute("href", "/providers/3/overview");
	});

	it("模型 detail 未返回时只展示到供应商列表层（链随 detail 就绪再补全）", () => {
		mocks.pathname = "/models/11/overview";
		mocks.detail = undefined;
		render(<Breadcrumbs />);
		const links = screen.getAllByRole("link");
		expect(links).toHaveLength(2);
		expect(links[1]).toHaveTextContent("供应商");
		expect(links[1]).toHaveAttribute("href", "/providers");
	});
});
