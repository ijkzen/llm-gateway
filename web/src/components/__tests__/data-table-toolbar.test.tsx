import { DataTableToolbar } from "@/components/data-table-toolbar";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

describe("DataTableToolbar", () => {
	it("renders children", () => {
		render(
			<DataTableToolbar>
				<button type="button">Action</button>
			</DataTableToolbar>,
		);
		expect(screen.getByText("Action")).toBeInTheDocument();
	});
});
