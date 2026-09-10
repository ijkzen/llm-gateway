import ProvidersPage from "@/pages/providers";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	providers: [] as Array<Record<string, unknown>>,
	isLoading: false,
	isError: false,
	refetch: vi.fn(),
}));

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useProviders: () => ({
			data: mocks.providers,
			isLoading: mocks.isLoading,
			isError: mocks.isError,
			refetch: mocks.refetch,
		}),
		useUpdateProvider: () => ({ mutate: vi.fn(), isPending: false }),
		useDeleteProvider: () => ({ mutate: vi.fn(), isPending: false }),
		useReorderProviders: () => ({ mutate: vi.fn() }),
	};
});

vi.mock("@/hooks/use-usage-estimate", () => ({
	useUsageEstimate: () => ({ data: undefined, isLoading: false }),
}));

function makeProvider(overrides: Record<string, unknown> = {}) {
	return {
		id: 1,
		name: "Alpha",
		baseUrl: "https://api.alpha.com",
		apiKeyMasked: "sk-****",
		enable: true,
		protocolType: 0,
		billingMode: 0,
		extra: "{}",
		customHeader: "{}",
		proxyEnabled: false,
		proxyAddr: "",
		sortOrder: 0,
		createdAt: "2026-01-01T00:00:00Z",
		updatedAt: "2026-01-01T00:00:00Z",
		...overrides,
	};
}

function renderPage() {
	const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
	return render(
		<QueryClientProvider client={queryClient}>
			<MemoryRouter>
				<ProvidersPage />
			</MemoryRouter>
		</QueryClientProvider>,
	);
}

describe("ProvidersPage 选中态", () => {
	beforeEach(() => {
		mocks.providers = [
			makeProvider({ id: 1, name: "Alpha" }),
			makeProvider({ id: 2, name: "Bravo", baseUrl: "https://api.bravo.com" }),
		];
		mocks.isLoading = false;
		mocks.isError = false;
	});

	it("未手动选择时默认展示列表首个供应商的详情", () => {
		renderPage();
		// 列表里两个名称，详情标题区展示当前选中的 Base URL。
		expect(screen.getAllByText("Alpha").length).toBeGreaterThan(0);
		expect(screen.getByText("https://api.alpha.com")).toBeTruthy();
	});

	it("手动选择第二个后详情切换", () => {
		renderPage();
		fireEvent.click(screen.getByText("Bravo"));
		expect(screen.getByText("https://api.bravo.com")).toBeTruthy();
	});

	it("删除当前选中项后回落到列表首个（17-11 回归）", () => {
		const { rerender } = renderPage();
		// 先手动选中第二个。
		fireEvent.click(screen.getByText("Bravo"));
		expect(screen.getByText("https://api.bravo.com")).toBeTruthy();

		// 该供应商被删除：列表刷新后不再包含它。
		mocks.providers = [makeProvider({ id: 1, name: "Alpha" })];
		rerender(
			<QueryClientProvider
				client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
			>
				<MemoryRouter>
					<ProvidersPage />
				</MemoryRouter>
			</QueryClientProvider>,
		);

		// 右侧详情不应停留在空白态，而是回落展示剩余的首个供应商。
		expect(screen.getByText("https://api.alpha.com")).toBeTruthy();
	});
});
