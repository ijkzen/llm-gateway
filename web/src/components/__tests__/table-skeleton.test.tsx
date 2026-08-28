import { TableSkeleton } from "@/components/table-skeleton";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

describe("TableSkeleton", () => {
	it("renders requested number of columns and rows", () => {
		render(<TableSkeleton columns={3} rows={2} />);
		// 1 header row + 2 body rows = 3 rows
		const rows = screen.getAllByRole("row");
		expect(rows).toHaveLength(3);
	});
});
