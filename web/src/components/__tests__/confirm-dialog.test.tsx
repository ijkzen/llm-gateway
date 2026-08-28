import { ConfirmDialog } from "@/components/confirm-dialog";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

describe("ConfirmDialog", () => {
	it("renders default Chinese copy and calls handleConfirm", () => {
		const handleConfirm = vi.fn();
		render(<ConfirmDialog open onOpenChange={() => {}} handleConfirm={handleConfirm} />);

		expect(screen.getByText("确认删除")).toBeInTheDocument();
		expect(screen.getByText("此操作无法撤销。")).toBeInTheDocument();

		fireEvent.click(screen.getByRole("button", { name: "确认" }));
		expect(handleConfirm).toHaveBeenCalledTimes(1);
	});

	it("renders custom desc and destructive confirm button", () => {
		render(
			<ConfirmDialog
				open
				onOpenChange={() => {}}
				desc={
					<>
						确定要删除任务 <span className="font-medium">demo</span> 吗？
					</>
				}
				confirmText="删除"
				destructive
				handleConfirm={() => {}}
			/>,
		);

		expect(screen.getByText("demo")).toBeInTheDocument();
		const confirmButton = screen.getByRole("button", { name: "删除" });
		expect(confirmButton.className).toContain("bg-destructive");
	});

	it("closes via cancel button", () => {
		const onOpenChange = vi.fn();
		render(<ConfirmDialog open onOpenChange={onOpenChange} handleConfirm={() => {}} />);

		fireEvent.click(screen.getByRole("button", { name: "取消" }));
		expect(onOpenChange).toHaveBeenCalledWith(false);
	});
});
