import { ChangePasswordDialog } from "@/components/settings/ChangePasswordDialog";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mutateMock = vi.fn();
const toastSuccess = vi.fn();
const toastError = vi.fn();

vi.mock("@/hooks/use-auth", () => ({
	useChangePassword: () => ({ mutate: mutateMock, isPending: false }),
}));

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({ toastSuccess, toastError }),
}));

function renderDialog(onOpenChange = vi.fn()) {
	return { onOpenChange, ...render(<ChangePasswordDialog open onOpenChange={onOpenChange} />) };
}

function fillAndSubmit(oldPassword: string, newPassword: string, confirmPassword: string) {
	fireEvent.change(screen.getByLabelText("旧密码"), { target: { value: oldPassword } });
	fireEvent.change(screen.getByLabelText("新密码"), { target: { value: newPassword } });
	fireEvent.change(screen.getByLabelText("确认新密码"), { target: { value: confirmPassword } });
	fireEvent.click(screen.getByRole("button", { name: "确认修改" }));
}

describe("ChangePasswordDialog", () => {
	beforeEach(() => {
		mutateMock.mockReset();
		toastSuccess.mockReset();
		toastError.mockReset();
	});

	it("渲染三个密码输入框", () => {
		renderDialog();

		expect(screen.getByLabelText("旧密码")).toBeInTheDocument();
		expect(screen.getByLabelText("新密码")).toBeInTheDocument();
		expect(screen.getByLabelText("确认新密码")).toBeInTheDocument();
	});

	it("两次新密码不一致时不提交并提示错误", async () => {
		renderDialog();

		fillAndSubmit("old-pass", "new-pass-1", "new-pass-2");

		await waitFor(() => {
			expect(screen.getByText("两次输入的新密码不一致")).toBeInTheDocument();
		});
		expect(mutateMock).not.toHaveBeenCalled();
	});

	it("校验通过后提交并提示成功、关闭弹窗", async () => {
		const onOpenChange = vi.fn();
		mutateMock.mockImplementation((_values, options) => options.onSuccess());
		renderDialog(onOpenChange);

		fillAndSubmit("old-pass", "new-pass-1", "new-pass-1");

		await waitFor(() => {
			expect(mutateMock).toHaveBeenCalledWith(
				{ oldPassword: "old-pass", newPassword: "new-pass-1" },
				expect.objectContaining({ onSuccess: expect.any(Function) }),
			);
		});
		expect(toastSuccess).toHaveBeenCalledWith("密码修改成功");
		expect(onOpenChange).toHaveBeenCalledWith(false);
	});

	it("提交失败时提示错误且不关闭", async () => {
		const onOpenChange = vi.fn();
		mutateMock.mockImplementation((_values, options) => options.onError(new Error("旧密码不正确")));
		renderDialog(onOpenChange);

		fillAndSubmit("wrong-old", "new-pass-1", "new-pass-1");

		await waitFor(() => {
			expect(toastError).toHaveBeenCalledWith("修改密码失败", expect.any(Error));
		});
		expect(onOpenChange).not.toHaveBeenCalledWith(false);
	});
});
