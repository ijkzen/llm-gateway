import type { RaceWindowState } from "@/components/race-window-control";
import { StatsSection } from "@/components/stats-section";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const DAY_STATE: RaceWindowState = {
	period: "day",
	offset: 0,
	customStart: 0,
	customEnd: 0,
	appliedCustom: null,
};

function Harness({
	status,
	onWindowChange = vi.fn(),
}: {
	status?: { isLoading: boolean; isError: boolean; onRetry: () => void };
	onWindowChange?: (patch: Partial<RaceWindowState>) => void;
}) {
	return (
		<StatsSection
			now={new Date(2026, 7, 30, 10).getTime()}
			windowState={DAY_STATE}
			onWindowChange={onWindowChange}
			status={status}
			windowTestId="call-window"
		>
			<div data-testid="content">chart</div>
		</StatsSection>
	);
}

describe("StatsSection 区块", () => {
	it("渲染副标题与内容（无状态时页面级门控）", () => {
		render(<Harness />);
		expect(screen.getByText("2026年8月30日（当前）")).toBeInTheDocument();
		expect(screen.getByTestId("content")).toBeInTheDocument();
	});

	it("载入中显示骨架，不渲染内容", () => {
		render(<Harness status={{ isLoading: true, isError: false, onRetry: vi.fn() }} />);
		expect(document.querySelector(".animate-pulse")).not.toBeNull();
		expect(screen.queryByTestId("content")).not.toBeInTheDocument();
	});

	it("错误态点击重试触发 onRetry", () => {
		const onRetry = vi.fn();
		render(<Harness status={{ isLoading: false, isError: true, onRetry }} />);
		fireEvent.click(screen.getByText("重试"));
		expect(onRetry).toHaveBeenCalledTimes(1);
	});

	it("窗口控件切下一周期触发 onWindowChange", () => {
		const onWindowChange = vi.fn();
		render(<Harness onWindowChange={onWindowChange} />);
		fireEvent.click(screen.getByRole("button", { name: "下一周期" }));
		expect(onWindowChange).toHaveBeenCalledWith({ offset: 1 });
	});

	it("副标题行携带测试锚点", () => {
		render(<Harness />);
		expect(screen.getByTestId("call-window")).toBeInTheDocument();
	});
});
