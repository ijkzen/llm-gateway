import { TooltipProvider } from "@/components/ui/tooltip";
import type { ApiKey, ApiKeyDetail } from "@/hooks/use-api-keys";
import ApiKeysPage from "@/pages/api-keys";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		apiKeys: undefined as ApiKey[] | undefined,
		detail: undefined as ApiKeyDetail | undefined,
		isLoading: false,
		isError: false,
		refetch: vi.fn(),
		createDialogOpen: false,
		deleteDialogOpen: false,
		toggleMutate: vi.fn(),
		deleteMutate: vi.fn(),
		writeText: vi.fn().mockResolvedValue(undefined),
	};
});

vi.mock("@/hooks/use-api-keys", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-api-keys")>("@/hooks/use-api-keys");
	return {
		...actual,
		useApiKeys: () => ({
			data: mocks.apiKeys,
			isLoading: mocks.isLoading,
			isError: mocks.isError,
			refetch: mocks.refetch,
		}),
		// 仅在展示明文（id 非空）时返回详情，模拟 query 缓存行为。
		useApiKeyDetail: (id: number | null) => ({
			data: id !== null ? mocks.detail : undefined,
			isLoading: false,
		}),
		useCreateApiKey: () => ({ mutate: vi.fn(), isPending: false }),
		useToggleApiKey: () => ({ mutate: mocks.toggleMutate, isPending: false }),
		useDeleteApiKey: () => ({ mutate: mocks.deleteMutate, isPending: false }),
	};
});

vi.mock("@/components/api-keys/ApiKeyCreateDialog", () => ({
	ApiKeyCreateDialog: (props: { open: boolean }) => {
		mocks.createDialogOpen = props.open;
		return null;
	},
}));

vi.mock("@/components/api-keys/ApiKeyDeleteDialog", () => ({
	ApiKeyDeleteDialog: (props: { open: boolean }) => {
		mocks.deleteDialogOpen = props.open;
		return null;
	},
}));

function makeKey(overrides: Partial<ApiKey> = {}): ApiKey {
	return {
		id: 1,
		name: "my-key",
		keyMasked: "lg-****abcd",
		enable: true,
		createdAt: "2026-08-29T00:00:00Z",
		updatedAt: "2026-08-29T00:00:00Z",
		...overrides,
	};
}

function makeDetail(overrides: Partial<ApiKeyDetail> = {}): ApiKeyDetail {
	return {
		...makeKey(),
		key: "lg-0123456789abcdef0123456789abcdef",
		...overrides,
	};
}

function renderPage() {
	const queryClient = new QueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<TooltipProvider>
				<MemoryRouter>
					<ApiKeysPage />
				</MemoryRouter>
			</TooltipProvider>
		</QueryClientProvider>,
	);
}

describe("ApiKeysPage", () => {
	beforeEach(() => {
		mocks.apiKeys = undefined;
		mocks.detail = undefined;
		mocks.isLoading = false;
		mocks.isError = false;
		mocks.createDialogOpen = false;
		mocks.deleteDialogOpen = false;
		mocks.toggleMutate.mockClear();
		mocks.deleteMutate.mockClear();
		mocks.writeText.mockClear();
		Object.defineProperty(navigator, "clipboard", {
			value: { writeText: mocks.writeText },
			configurable: true,
		});
	});

	it("加载中渲染骨架屏，不渲染内容", () => {
		mocks.isLoading = true;
		renderPage();

		expect(screen.queryByText(/暂无 API Key/)).toBeNull();
		expect(screen.queryByRole("button", { name: "创建 API Key" })).toBeNull();
	});

	it("加载失败展示错误态", () => {
		mocks.isError = true;
		renderPage();

		expect(screen.getByText(/无法获取 API Key 数据/)).toBeTruthy();
	});

	it("空态文案 + 创建按钮打开创建弹窗", () => {
		mocks.apiKeys = [];
		renderPage();

		expect(screen.getByText(/暂无 API Key/)).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: "创建 API Key" }));
		expect(mocks.createDialogOpen).toBe(true);
	});

	it("列表行展示名称与掩码 key，默认不出现明文", () => {
		mocks.apiKeys = [makeKey()];
		renderPage();

		expect(screen.getByText("my-key")).toBeTruthy();
		expect(screen.getByText("lg-****abcd")).toBeTruthy();
		expect(screen.queryByText("lg-0123456789abcdef0123456789abcdef")).toBeNull();
	});

	it("小眼睛切换明文展示", async () => {
		mocks.apiKeys = [makeKey()];
		mocks.detail = makeDetail();
		renderPage();

		fireEvent.click(screen.getByRole("button", { name: "显示 API Key" }));
		await screen.findByText("lg-0123456789abcdef0123456789abcdef");

		fireEvent.click(screen.getByRole("button", { name: "隐藏 API Key" }));
		await waitFor(() =>
			expect(screen.queryByText("lg-0123456789abcdef0123456789abcdef")).toBeNull(),
		);
		expect(screen.getByText("lg-****abcd")).toBeTruthy();
	});

	it("一键复制：明文写入剪贴板", async () => {
		mocks.apiKeys = [makeKey()];
		mocks.detail = makeDetail();
		renderPage();

		// 先展开明文再复制，走已加载详情分支。
		fireEvent.click(screen.getByRole("button", { name: "显示 API Key" }));
		await screen.findByText("lg-0123456789abcdef0123456789abcdef");
		fireEvent.click(screen.getByRole("button", { name: "复制 API Key" }));

		await waitFor(() =>
			expect(mocks.writeText).toHaveBeenCalledWith("lg-0123456789abcdef0123456789abcdef"),
		);
	});

	it("状态开关切换启用/禁用", () => {
		mocks.apiKeys = [makeKey()];
		renderPage();

		fireEvent.click(screen.getByRole("switch", { name: "切换 API Key my-key 状态" }));
		expect(mocks.toggleMutate).toHaveBeenCalledWith(
			{ id: 1, enable: false },
			expect.objectContaining({ onSuccess: expect.any(Function) }),
		);
	});

	it("操作菜单：点「禁用」切换状态", async () => {
		mocks.apiKeys = [makeKey()];
		renderPage();

		// Radix DropdownMenu 在 jsdom 下通过键盘事件打开。
		fireEvent.keyDown(screen.getByRole("button", { name: "操作 my-key" }), { key: "Enter" });
		fireEvent.click(await screen.findByRole("menuitem", { name: "禁用" }));
		expect(mocks.toggleMutate).toHaveBeenCalledWith({ id: 1, enable: false }, expect.anything());
	});

	it("操作菜单：点「删除」打开删除确认弹窗", async () => {
		mocks.apiKeys = [makeKey()];
		renderPage();

		fireEvent.keyDown(screen.getByRole("button", { name: "操作 my-key" }), { key: "Enter" });
		fireEvent.click(await screen.findByRole("menuitem", { name: "删除" }));
		await waitFor(() => expect(mocks.deleteDialogOpen).toBe(true));
	});
});
