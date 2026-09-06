import { RaceCardShell, useRaceCardWindow } from "@/components/race-card-shell";
import { fireEvent, render, screen } from "@testing-library/react";
import { Boxes } from "lucide-react";
import { describe, expect, it, vi } from "vitest";

function Harness({
	isLoading = false,
	isError = false,
	isEmpty = false,
	forceInView = true,
	onRetry = vi.fn(),
}: {
	isLoading?: boolean;
	isError?: boolean;
	isEmpty?: boolean;
	forceInView?: boolean;
	onRetry?: () => void;
}) {
	const view = useRaceCardWindow();
	return (
		<RaceCardShell
			view={{ ...view, inView: forceInView }}
			icon={Boxes}
			titleKey="dashboard.providerRace"
			status={{ isLoading, isError, isEmpty, onRetry }}
		>
			<div data-testid="content">rows</div>
		</RaceCardShell>
	);
}

describe("RaceCardShell 状态分支", () => {
	it("未进视口显示懒加载提示", () => {
		// stub IntersectionObserver：从不回调 → inView 保持 false。
		vi.stubGlobal(
			"IntersectionObserver",
			class {
				observe() {}
				disconnect() {}
				unobserve() {}
			},
		);
		try {
			render(<Harness forceInView={false} />);
			expect(screen.getByText("滚动到此处后加载")).toBeInTheDocument();
			expect(screen.queryByTestId("content")).not.toBeInTheDocument();
		} finally {
			vi.unstubAllGlobals();
		}
	});

	it("载入中显示骨架", () => {
		render(<Harness isLoading />);
		expect(document.querySelector(".animate-pulse")).not.toBeNull();
		expect(screen.queryByTestId("content")).not.toBeInTheDocument();
	});

	it("出错显示重试按钮并触发 onRetry", () => {
		const onRetry = vi.fn();
		render(<Harness isError onRetry={onRetry} />);
		fireEvent.click(screen.getByRole("button", { name: "重试" }));
		expect(onRetry).toHaveBeenCalledTimes(1);
	});

	it("空数据显示无数据提示", () => {
		render(<Harness isEmpty />);
		expect(screen.getByText("该时间段暂无数据")).toBeInTheDocument();
		expect(screen.queryByTestId("content")).not.toBeInTheDocument();
	});

	it("正常态渲染标题与内容", () => {
		render(<Harness />);
		expect(screen.getByTestId("content")).toBeInTheDocument();
		expect(screen.getByText("供应商赛马")).toBeInTheDocument();
	});
});
