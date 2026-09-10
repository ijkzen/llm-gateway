import { TooltipProvider } from "@/components/ui/tooltip";
import SettingsPage from "@/pages/settings";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	settings: [] as Array<Record<string, unknown>>,
	isLoading: false,
	isError: false,
	refetch: vi.fn(),
}));

vi.mock("@/hooks/use-settings", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-settings")>("@/hooks/use-settings");
	return {
		...actual,
		useSettings: () => ({
			data: mocks.settings,
			isLoading: mocks.isLoading,
			isError: mocks.isError,
			refetch: mocks.refetch,
		}),
	};
});

vi.mock("@/hooks/use-auth", () => ({
	useChangePassword: () => ({ mutate: vi.fn(), isPending: false }),
}));

function makeSetting(key: string, type: string, value: string) {
	return { key, value, type, updated_at: "2026-08-01T00:00:00Z" };
}

function renderPage() {
	const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
	return render(
		<QueryClientProvider client={queryClient}>
			<TooltipProvider>
				<MemoryRouter>
					<SettingsPage />
				</MemoryRouter>
			</TooltipProvider>
		</QueryClientProvider>,
	);
}

describe("SettingsPage 弹窗编排", () => {
	beforeEach(() => {
		mocks.settings = [
			makeSetting("language", "String", "zh-CN"),
			makeSetting("downstream_request_header_allow_list", "Json", '["traceparent"]'),
			makeSetting("max_consecutive_failures", "Int", "5"),
		];
		mocks.isLoading = false;
		mocks.isError = false;
	});

	it("String 类型的编辑走单值弹窗并展示 key 与类型（18-12）", () => {
		renderPage();
		fireEvent.keyDown(screen.getByRole("button", { name: "操作 language" }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitem", { name: "编辑" }));

		// 弹窗头显示 key 与类型（表格行里也有类型徽章，故用角色限定到 dialog）。
		const dialog = screen.getByRole("dialog");
		expect(within(dialog).getByText("String")).toBeTruthy();
		expect(within(dialog).getByDisplayValue("zh-CN")).toBeTruthy();
	});

	it("Int 类型编辑走单值弹窗（18-04：不再按纯文本放行非法值）", () => {
		renderPage();
		fireEvent.keyDown(screen.getByRole("button", { name: "操作 max_consecutive_failures" }), {
			key: "ArrowDown",
		});
		fireEvent.click(screen.getByRole("menuitem", { name: "编辑" }));

		const dialog = screen.getByRole("dialog");
		expect(within(dialog).getByText("Int")).toBeTruthy();
		expect(within(dialog).getByDisplayValue("5")).toBeTruthy();
	});

	it("Json 类型的编辑路由到结构化弹窗（逐行增删键值）", () => {
		renderPage();
		fireEvent.keyDown(
			screen.getByRole("button", { name: "操作 downstream_request_header_allow_list" }),
			{ key: "ArrowDown" },
		);
		fireEvent.click(screen.getByRole("menuitem", { name: "编辑" }));

		expect(screen.getByText("编辑 JSON 设置")).toBeTruthy();
		expect(screen.getByDisplayValue("traceparent")).toBeTruthy();
	});

	it("内置键 language 的删除入口被禁用（18-20）", () => {
		renderPage();
		fireEvent.keyDown(screen.getByRole("button", { name: "操作 language" }), { key: "ArrowDown" });
		const deleteItem = screen.getByRole("menuitem", { name: "删除" });
		expect(deleteItem.getAttribute("aria-disabled")).toBe("true");
	});

	it("加载失败展示错误态并可重试", () => {
		mocks.isError = true;
		renderPage();
		expect(screen.getByRole("button", { name: /重试/ })).toBeTruthy();
		fireEvent.click(screen.getByRole("button", { name: /重试/ }));
		expect(mocks.refetch).toHaveBeenCalled();
	});
});
