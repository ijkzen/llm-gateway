import { SkipToMain } from "@/components/skip-to-main";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

describe("SkipToMain", () => {
	it("renders a link pointing to the main content anchor", () => {
		render(<SkipToMain />);

		const link = screen.getByRole("link", { name: "跳到主内容" });
		expect(link).toBeInTheDocument();
		expect(link).toHaveAttribute("href", "#content");
	});
});
