import { SettingsTable } from "@/components/settings/SettingsTable";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Setting } from "@/hooks/use-settings";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

function makeSetting(key: string, overrides: Partial<Setting> = {}): Setting {
	return {
		key,
		value: `${key}-value`,
		type: "String",
		updated_at: "2026-08-01T00:00:00Z",
		...overrides,
	};
}

function renderTable(settings: Setting[] | undefined, onEdit = vi.fn()) {
	return {
		onEdit,
		...render(
			<TooltipProvider>
				<SettingsTable settings={settings} onEdit={onEdit} />
			</TooltipProvider>,
		),
	};
}

function getDataRows() {
	return screen.getAllByRole("row").slice(1);
}

function getDataRowAt(index: number) {
	const row = getDataRows()[index];
	if (!row) throw new Error(`data row ${index} not found`);
	return row;
}

describe("SettingsTable", () => {
	it("renders settings rows with type badges", () => {
		renderTable([makeSetting("a"), makeSetting("b", { type: "Bool" })]);

		expect(screen.getByText("a")).toBeInTheDocument();
		expect(screen.getByText("b")).toBeInTheDocument();
		expect(screen.getByText("String")).toBeInTheDocument();
		expect(screen.getByText("Bool")).toBeInTheDocument();
	});

	it("shows empty state when there are no settings", () => {
		renderTable([]);

		expect(screen.getByText("暂无设置项")).toBeInTheDocument();
	});

	it("sorts rows by key via the column header menu", () => {
		renderTable([makeSetting("b"), makeSetting("a"), makeSetting("c")]);

		fireEvent.keyDown(screen.getByRole("button", { name: "键" }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitem", { name: "升序" }));

		expect(within(getDataRowAt(0)).getByText("a")).toBeInTheDocument();
		expect(within(getDataRowAt(1)).getByText("b")).toBeInTheDocument();
		expect(within(getDataRowAt(2)).getByText("c")).toBeInTheDocument();
	});

	it("paginates with 10 rows per page by default", () => {
		const settings = Array.from({ length: 25 }, (_, i) =>
			makeSetting(`key-${String(i).padStart(2, "0")}`),
		);
		renderTable(settings);

		expect(getDataRows()).toHaveLength(10);
		expect(within(getDataRowAt(0)).getByText("key-00")).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "下一页" }));

		expect(getDataRows()).toHaveLength(10);
		expect(within(getDataRowAt(0)).getByText("key-10")).toBeInTheDocument();
		expect(screen.getByText("第 2 / 3 页")).toBeInTheDocument();
	});

	it("hides a column via view options", () => {
		renderTable([makeSetting("a")]);

		expect(screen.getByText("值")).toBeInTheDocument();

		fireEvent.keyDown(screen.getByRole("button", { name: "显示列" }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "值" }));

		expect(screen.queryByText("值")).not.toBeInTheDocument();
	});

	it("calls onEdit from the row action menu", () => {
		const setting = makeSetting("a");
		const { onEdit } = renderTable([setting]);

		fireEvent.keyDown(screen.getByRole("button", { name: "操作 a" }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitem", { name: "编辑" }));

		expect(onEdit).toHaveBeenCalledWith(setting);
	});
});
