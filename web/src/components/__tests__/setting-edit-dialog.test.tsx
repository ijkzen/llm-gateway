import { SettingEditDialog } from "@/components/settings/SettingEditDialog";
import type { Setting } from "@/hooks/use-settings";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	updateMutate: vi.fn(),
}));

vi.mock("@/hooks/use-settings", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-settings")>("@/hooks/use-settings");
	return {
		...actual,
		useUpdateSetting: () => ({ mutate: mocks.updateMutate, isPending: false }),
	};
});

function makeSetting(type: string, value: string): Setting {
	return { key: "max_consecutive_failures", value, type: type as Setting["type"], updated_at: "" };
}

function renderDialog(setting: Setting) {
	const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
	return render(
		<QueryClientProvider client={queryClient}>
			<SettingEditDialog setting={setting} open onOpenChange={() => {}} />
		</QueryClientProvider>,
	);
}

describe("SettingEditDialog 类型校验（18-04 回归）", () => {
	beforeEach(() => {
		mocks.updateMutate.mockClear();
	});

	it("Int 类型：非整数值当场标错，不提交", async () => {
		renderDialog(makeSetting("Int", "5"));
		const dialog = screen.getByRole("dialog");
		const input = within(dialog).getByDisplayValue("5");

		fireEvent.change(input, { target: { value: "abc" } });
		fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

		await waitFor(() => expect(screen.getByText("必须是有效的整数")).toBeTruthy());
		expect(mocks.updateMutate).not.toHaveBeenCalled();
	});

	it("Int 类型：合法整数正常提交", async () => {
		renderDialog(makeSetting("Int", "5"));
		const dialog = screen.getByRole("dialog");
		fireEvent.change(within(dialog).getByDisplayValue("5"), { target: { value: "10" } });
		fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

		await waitFor(() =>
			expect(mocks.updateMutate).toHaveBeenCalledWith(
				{ key: "max_consecutive_failures", value: "10" },
				expect.anything(),
			),
		);
	});

	it("Bool 类型：渲染开关控件而非文本输入", () => {
		renderDialog(makeSetting("Bool", "true"));
		const dialog = screen.getByRole("dialog");
		expect(within(dialog).getByRole("switch")).toBeTruthy();
		expect(within(dialog).queryByRole("textbox")).toBeNull();
	});

	it("Float 类型：非法数值标错", async () => {
		renderDialog(makeSetting("Float", "1.5"));
		const dialog = screen.getByRole("dialog");
		fireEvent.change(within(dialog).getByDisplayValue("1.5"), { target: { value: "x" } });
		fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));

		await waitFor(() => expect(screen.getByText("必须是有效的数字")).toBeTruthy());
		expect(mocks.updateMutate).not.toHaveBeenCalled();
	});

	it("弹窗头展示当前编辑的 key 与类型（18-12）", () => {
		renderDialog(makeSetting("Int", "5"));
		const dialog = screen.getByRole("dialog");
		expect(within(dialog).getByText("max_consecutive_failures")).toBeTruthy();
		expect(within(dialog).getByText("Int")).toBeTruthy();
	});
});
