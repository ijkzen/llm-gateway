import { JsonSettingEditDialog } from "@/components/settings/JsonSettingEditDialog";
import type { Setting } from "@/hooks/use-settings";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	updateMutate: vi.fn(),
	toastSuccess: vi.fn(),
	toastError: vi.fn(),
}));

vi.mock("@/hooks/use-settings", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-settings")>("@/hooks/use-settings");
	return {
		...actual,
		useUpdateSetting: () => ({ mutate: mocks.updateMutate, isPending: false }),
	};
});

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({
		toastSuccess: mocks.toastSuccess,
		toastError: mocks.toastError,
	}),
}));

function makeSetting(value: string): Setting {
	return {
		key: "downstream_request_header_allow_list",
		value,
		type: "Json",
		updated_at: "2026-09-05T00:00:00Z",
	};
}

beforeEach(() => {
	vi.clearAllMocks();
});

describe("JsonSettingEditDialog", () => {
	it("数组模式：逐行展示、删除条目后保存为紧凑 JSON", () => {
		render(
			<JsonSettingEditDialog
				open
				onOpenChange={vi.fn()}
				setting={makeSetting('["traceparent","user-agent"]')}
			/>,
		);
		expect(screen.getByDisplayValue("traceparent")).toBeInTheDocument();
		expect(screen.getByDisplayValue("user-agent")).toBeInTheDocument();

		// 删除第一行（traceparent），保存。
		fireEvent.click(screen.getAllByRole("button", { name: "删除" })[0]);
		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		expect(mocks.updateMutate).toHaveBeenCalledWith(
			{ key: "downstream_request_header_allow_list", value: '["user-agent"]' },
			expect.anything(),
		);
	});

	it("新增行并填写后保存会追加条目", () => {
		render(
			<JsonSettingEditDialog
				open
				onOpenChange={vi.fn()}
				setting={makeSetting('["traceparent"]')}
			/>,
		);
		fireEvent.click(screen.getByRole("button", { name: /新增/ }));
		const inputs = screen.getAllByPlaceholderText("值");
		fireEvent.change(inputs[inputs.length - 1], {
			target: { value: "x-new-header" },
		});
		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		expect(mocks.updateMutate).toHaveBeenCalledWith(
			{
				key: "downstream_request_header_allow_list",
				value: '["traceparent","x-new-header"]',
			},
			expect.anything(),
		);
	});

	it("对象模式：展示键值两列，空键行保存时被丢弃", () => {
		render(
			<JsonSettingEditDialog
				open
				onOpenChange={vi.fn()}
				setting={makeSetting('{"X-A":"1","X-B":"2"}')}
			/>,
		);
		expect(screen.getByDisplayValue("X-A")).toBeInTheDocument();
		expect(screen.getByDisplayValue("X-B")).toBeInTheDocument();

		// 清空第一行键 → 保存时该行丢弃。
		fireEvent.change(screen.getByDisplayValue("X-A"), { target: { value: "" } });
		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		expect(mocks.updateMutate).toHaveBeenCalledWith(
			{ key: "downstream_request_header_allow_list", value: '{"X-B":"2"}' },
			expect.anything(),
		);
	});

	it("嵌套 JSON 退回原文编辑并原样保存", () => {
		render(
			<JsonSettingEditDialog
				open
				onOpenChange={vi.fn()}
				setting={makeSetting('{"a":["nested"]}')}
			/>,
		);
		// 无逐行编辑控件，出现原文文本域。
		expect(screen.queryByRole("button", { name: /新增/ })).not.toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: "保存" }));
		expect(mocks.updateMutate).toHaveBeenCalledWith(
			{ key: "downstream_request_header_allow_list", value: '{"a":["nested"]}' },
			expect.anything(),
		);
	});
});
