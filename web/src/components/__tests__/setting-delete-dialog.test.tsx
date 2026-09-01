import { SettingDeleteDialog } from "@/components/settings/SettingDeleteDialog";
import type { Setting } from "@/hooks/use-settings";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const deleteMutate = vi.fn();
const toastSuccess = vi.fn();
const toastError = vi.fn();

vi.mock("@/hooks/use-settings", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-settings")>("@/hooks/use-settings");
	return {
		...actual,
		useDeleteSetting: () => ({ mutate: deleteMutate, isPending: false }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({ toastSuccess, toastError }),
}));

const setting: Setting = {
	key: "site_name",
	value: "My Site",
	type: "String",
	updated_at: "2026-08-01T00:00:00Z",
};

function renderDialog(onOpenChange = vi.fn()) {
	return {
		onOpenChange,
		...render(<SettingDeleteDialog setting={setting} open onOpenChange={onOpenChange} />),
	};
}

describe("SettingDeleteDialog", () => {
	beforeEach(() => {
		deleteMutate.mockReset();
		toastSuccess.mockReset();
		toastError.mockReset();
	});

	it("展示设置 key 与二次确认文案", () => {
		renderDialog();

		expect(screen.getByText("删除设置")).toBeInTheDocument();
		expect(screen.getByText("site_name")).toBeInTheDocument();
		expect(screen.getByText(/确定要删除设置项/)).toBeInTheDocument();
	});

	it("确认删除后调用删除接口并提示成功、关闭弹窗", () => {
		const onOpenChange = vi.fn();
		deleteMutate.mockImplementation((_key, options) => options.onSuccess());
		renderDialog(onOpenChange);

		fireEvent.click(screen.getByRole("button", { name: "删除" }));

		expect(deleteMutate).toHaveBeenCalledWith("site_name", expect.anything());
		expect(toastSuccess).toHaveBeenCalledWith("删除成功");
		expect(onOpenChange).toHaveBeenCalledWith(false);
	});

	it("删除失败时提示失败且不关闭弹窗", () => {
		const onOpenChange = vi.fn();
		deleteMutate.mockImplementation((_key, options) =>
			options.onError({ message: "内置设置不可删除" }),
		);
		renderDialog(onOpenChange);

		fireEvent.click(screen.getByRole("button", { name: "删除" }));

		expect(toastError).toHaveBeenCalledWith("删除失败", { message: "内置设置不可删除" });
		expect(onOpenChange).not.toHaveBeenCalled();
	});
});
