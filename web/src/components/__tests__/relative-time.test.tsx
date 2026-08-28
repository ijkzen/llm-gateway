import { RelativeTime } from "@/components/relative-time";
import { TooltipProvider } from "@/components/ui/tooltip";
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it } from "vitest";

function renderWithTooltip(ui: ReactNode) {
	return render(<TooltipProvider>{ui}</TooltipProvider>);
}

describe("RelativeTime", () => {
	it("renders fallback for invalid date", () => {
		renderWithTooltip(<RelativeTime date="" fallback="等待执行" />);
		expect(screen.getByText("等待执行")).toBeInTheDocument();
	});

	it("renders relative time for recent date", () => {
		const recent = new Date(Date.now() - 2 * 60 * 1000).toISOString();
		renderWithTooltip(<RelativeTime date={recent} />);
		expect(screen.getByText("2 分钟前")).toBeInTheDocument();
	});

	it("does not crash when date toggles between valid and invalid on the same instance", () => {
		const recent = new Date(Date.now() - 2 * 60 * 1000).toISOString();
		const { rerender } = renderWithTooltip(<RelativeTime date={recent} />);
		expect(screen.getByText("2 分钟前")).toBeInTheDocument();

		rerender(
			<TooltipProvider>
				<RelativeTime date="invalid" />
			</TooltipProvider>,
		);
		expect(screen.getByText("—")).toBeInTheDocument();

		rerender(
			<TooltipProvider>
				<RelativeTime date={recent} />
			</TooltipProvider>,
		);
		expect(screen.getByText("2 分钟前")).toBeInTheDocument();
	});
});
