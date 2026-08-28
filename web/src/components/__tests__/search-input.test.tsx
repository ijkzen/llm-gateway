import { SearchInput } from "@/components/search-input";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

describe("SearchInput", () => {
	it("calls onChange when user types", () => {
		const handleChange = vi.fn();
		render(<SearchInput value="" onChange={handleChange} />);
		const input = screen.getByPlaceholderText("搜索...");
		fireEvent.change(input, { target: { value: "hello" } });
		expect(handleChange).toHaveBeenCalledWith("hello");
	});

	it("uses placeholder as the accessible name by default", () => {
		render(<SearchInput value="" onChange={vi.fn()} placeholder="搜索任务" />);
		expect(screen.getByRole("searchbox", { name: "搜索任务" })).toBeInTheDocument();
	});

	it("prefers aria-label as the accessible name when provided", () => {
		render(
			<SearchInput value="" onChange={vi.fn()} placeholder="搜索任务" aria-label="任务搜索框" />,
		);
		expect(screen.getByRole("searchbox", { name: "任务搜索框" })).toBeInTheDocument();
		expect(screen.queryByRole("searchbox", { name: "搜索任务" })).not.toBeInTheDocument();
	});
});
