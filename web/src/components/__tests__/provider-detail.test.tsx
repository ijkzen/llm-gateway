import { ProviderDetail } from "@/components/providers/ProviderDetail";
import type { Provider } from "@/hooks/use-providers";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		fetchProviderApiKey: vi.fn(),
		updateMutate: vi.fn(),
		writeText: vi.fn().mockResolvedValue(undefined),
		toastSuccess: vi.fn(),
		toastError: vi.fn(),
	};
});

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		fetchProviderApiKey: mocks.fetchProviderApiKey,
		useUpdateProvider: () => ({ mutate: mocks.updateMutate, isPending: false }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: mocks.toastSuccess,
		toastError: mocks.toastError,
	}),
}));

const provider: Provider = {
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
	createdAt: "2026-08-30T12:00:00Z",
	updatedAt: "2026-08-30T12:00:00Z",
};

function renderDetail() {
	render(<ProviderDetail provider={provider} onEdit={vi.fn()} onDelete={vi.fn()} />);
}

beforeEach(() => {
	vi.clearAllMocks();
	vi.stubGlobal("navigator", { clipboard: { writeText: mocks.writeText } });
});

describe("ProviderDetail 明文 API Key 展示", () => {
	it("默认展示脱敏后的 apiKeyMasked，不发请求", () => {
		renderDetail();
		expect(screen.getByText("sk-****test")).toBeInTheDocument();
		expect(mocks.fetchProviderApiKey).not.toHaveBeenCalled();
	});

	it("点小眼睛请求明文并展示；再点恢复脱敏（不发第二次请求）", async () => {
		mocks.fetchProviderApiKey.mockResolvedValue("sk-plain-1234");
		renderDetail();

		fireEvent.click(screen.getByRole("button", { name: "显示 API Key" }));
		expect(await screen.findByText("sk-plain-1234")).toBeInTheDocument();
		expect(mocks.fetchProviderApiKey).toHaveBeenCalledTimes(1);
		expect(mocks.fetchProviderApiKey).toHaveBeenCalledWith(7);

		// 已展示明文时再点眼睛 → 本地切回脱敏，不再发请求。
		fireEvent.click(screen.getByRole("button", { name: "隐藏 API Key" }));
		expect(screen.getByText("sk-****test")).toBeInTheDocument();
		expect(mocks.fetchProviderApiKey).toHaveBeenCalledTimes(1);
	});

	it("明文请求失败时提示错误并保持脱敏展示", async () => {
		mocks.fetchProviderApiKey.mockRejectedValue(new Error("network"));
		renderDetail();

		fireEvent.click(screen.getByRole("button", { name: "显示 API Key" }));
		await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
		expect(screen.getByText("sk-****test")).toBeInTheDocument();
	});

	it("一键复制总是重新请求明文，即使当前已展示明文", async () => {
		mocks.fetchProviderApiKey.mockResolvedValue("sk-plain-1234");
		renderDetail();

		// 先展开明文。
		fireEvent.click(screen.getByRole("button", { name: "显示 API Key" }));
		await screen.findByText("sk-plain-1234");
		expect(mocks.fetchProviderApiKey).toHaveBeenCalledTimes(1);

		// 复制：即便明文已在页面上，仍重新请求并写入剪贴板。
		fireEvent.click(screen.getByRole("button", { name: "复制 API Key" }));
		await waitFor(() => expect(mocks.fetchProviderApiKey).toHaveBeenCalledTimes(2));
		await waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith("sk-plain-1234"));
		expect(mocks.toastSuccess).toHaveBeenCalled();
	});

	it("复制失败时提示错误", async () => {
		mocks.fetchProviderApiKey.mockRejectedValue(new Error("network"));
		renderDetail();

		fireEvent.click(screen.getByRole("button", { name: "复制 API Key" }));
		await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
	});
});

describe("ProviderDetail 额外配置 / 自定义请求头折叠", () => {
	const extraConfigTitle = "额外配置";
	const customHeaderTitle = "自定义请求头";

	function renderWithData(overrides: Partial<Provider> = {}) {
		const p: Provider = {
			...provider,
			extra: '{"refresh_token": "rt-123"}',
			customHeader: '{"X-Custom": "abc"}',
			...overrides,
		};
		return render(<ProviderDetail provider={p} onEdit={vi.fn()} onDelete={vi.fn()} />);
	}

	it("默认折叠：只显示标题行，内容（键值输入框 / pre 块）不可见", () => {
		renderWithData();
		const extraBtn = screen.getByRole("button", { name: extraConfigTitle });
		const headerBtn = screen.getByRole("button", { name: customHeaderTitle });
		expect(extraBtn).toHaveAttribute("aria-expanded", "false");
		expect(headerBtn).toHaveAttribute("aria-expanded", "false");
		// 折叠时不渲染内容：额外配置键值输入框与自定义请求头 pre 内容都不在。
		expect(screen.queryByDisplayValue("rt-123")).not.toBeInTheDocument();
		expect(screen.queryByText(/X-Custom/)).not.toBeInTheDocument();
	});

	it("点标题行（整行按钮，方向键在按钮内）展开/收起，两个区独立", () => {
		renderWithData();
		const extraBtn = screen.getByRole("button", { name: extraConfigTitle });
		const headerBtn = screen.getByRole("button", { name: customHeaderTitle });

		fireEvent.click(extraBtn);
		expect(extraBtn).toHaveAttribute("aria-expanded", "true");
		expect(screen.getByDisplayValue("rt-123")).toBeInTheDocument();
		// 额外配置展开不影响自定义请求头。
		expect(headerBtn).toHaveAttribute("aria-expanded", "false");

		fireEvent.click(headerBtn);
		expect(headerBtn).toHaveAttribute("aria-expanded", "true");
		expect(screen.getByText(/X-Custom/)).toBeInTheDocument();

		// 再点标题行收起。
		fireEvent.click(extraBtn);
		expect(extraBtn).toHaveAttribute("aria-expanded", "false");
		expect(screen.queryByDisplayValue("rt-123")).not.toBeInTheDocument();
	});

	it("切换供应商时展开态重置回折叠", () => {
		const initial: Provider = { ...provider, extra: '{"refresh_token": "rt-123"}' };
		const other: Provider = { ...provider, id: 8, name: "Anthropic", extra: '{"k": "v"}' };
		const { rerender } = render(
			<ProviderDetail provider={initial} onEdit={vi.fn()} onDelete={vi.fn()} />,
		);

		fireEvent.click(screen.getByRole("button", { name: extraConfigTitle }));
		expect(screen.getByRole("button", { name: extraConfigTitle })).toHaveAttribute(
			"aria-expanded",
			"true",
		);

		rerender(<ProviderDetail provider={other} onEdit={vi.fn()} onDelete={vi.fn()} />);
		expect(screen.getByRole("button", { name: extraConfigTitle })).toHaveAttribute(
			"aria-expanded",
			"false",
		);
		expect(screen.queryByDisplayValue("v")).not.toBeInTheDocument();
	});

	it("extra/customHeader 为空对象时不渲染对应标题行", () => {
		renderWithData({ extra: "{}", customHeader: "{}" });
		expect(screen.queryByRole("button", { name: extraConfigTitle })).not.toBeInTheDocument();
		expect(screen.queryByRole("button", { name: customHeaderTitle })).not.toBeInTheDocument();
	});
});
