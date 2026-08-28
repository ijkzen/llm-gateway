import { ErrorState } from "@/components/error-state";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

describe("ErrorState", () => {
	it("renders default title and description", () => {
		render(<ErrorState description="无法获取数据，请稍后重试。" />);
		expect(screen.getByText("加载失败")).toBeInTheDocument();
		expect(screen.getByText("无法获取数据，请稍后重试。")).toBeInTheDocument();
	});

	it("renders custom title", () => {
		render(<ErrorState title="数据加载出错" />);
		expect(screen.getByText("数据加载出错")).toBeInTheDocument();
		expect(screen.queryByText("加载失败")).not.toBeInTheDocument();
	});

	it("calls onRetry when the retry button is clicked", () => {
		const onRetry = vi.fn();
		render(<ErrorState onRetry={onRetry} />);
		fireEvent.click(screen.getByRole("button", { name: "重试" }));
		expect(onRetry).toHaveBeenCalledTimes(1);
	});

	it("does not render the retry button without onRetry", () => {
		render(<ErrorState description="无法获取数据，请稍后重试。" />);
		expect(screen.queryByRole("button")).not.toBeInTheDocument();
	});
});
