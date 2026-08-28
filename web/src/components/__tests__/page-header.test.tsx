import { PageHeader } from "@/components/page-header";
import { render, screen } from "@testing-library/react";
import { Clock } from "lucide-react";
import { describe, expect, it } from "vitest";

describe("PageHeader", () => {
	it("renders title", () => {
		render(<PageHeader title="定时任务" />);
		expect(screen.getByText("定时任务")).toBeInTheDocument();
	});

	it("renders icon and children", () => {
		render(
			<PageHeader title="设置" icon={Clock}>
				<button type="button">操作</button>
			</PageHeader>,
		);
		expect(screen.getByText("设置")).toBeInTheDocument();
		expect(screen.getByText("操作")).toBeInTheDocument();
	});
});
