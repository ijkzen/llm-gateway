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
	status: 0,
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
