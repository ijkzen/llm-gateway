import LoginPage from "@/pages/login";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const useMeMock = vi.fn();
const useAuthStatusMock = vi.fn();
const loginMutateMock = vi.fn();
const initMutateMock = vi.fn();
const toastError = vi.fn();

vi.mock("@/hooks/use-auth", () => ({
	useMe: () => useMeMock(),
	useAuthStatus: () => useAuthStatusMock(),
	useLogin: () => ({ mutate: loginMutateMock, isPending: false }),
	useInitAdmin: () => ({ mutate: initMutateMock, isPending: false }),
}));

vi.mock("@/hooks/use-toast", () => ({
	useToastActions: () => ({ toastSuccess: vi.fn(), toastError }),
}));

function renderPage(state?: { from?: string }) {
	return render(
		<MemoryRouter initialEntries={[{ pathname: "/login", state }]}>
			<Routes>
				<Route path="/login" element={<LoginPage />} />
				<Route path="/" element={<div>首页</div>} />
				<Route path="/providers" element={<div>供应商页</div>} />
			</Routes>
		</MemoryRouter>,
	);
}

describe("LoginPage", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		useMeMock.mockReturnValue({ data: undefined, isLoading: false, isError: true });
	});

	it("未初始化时显示初始化表单（含确认密码）", () => {
		useAuthStatusMock.mockReturnValue({
			data: { initialized: false },
			isLoading: false,
			isError: false,
		});

		renderPage();

		expect(screen.getByText("初始化管理员")).toBeInTheDocument();
		expect(screen.getByLabelText("确认密码")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "创建管理员" })).toBeInTheDocument();
	});

	it("已初始化时显示登录表单", () => {
		useAuthStatusMock.mockReturnValue({
			data: { initialized: true },
			isLoading: false,
			isError: false,
		});

		renderPage();

		expect(screen.getByText("登录 LLM Gateway")).toBeInTheDocument();
		expect(screen.queryByLabelText("确认密码")).not.toBeInTheDocument();
		expect(screen.getByRole("button", { name: "登录" })).toBeInTheDocument();
	});

	it("初始化提交成功后回跳来源页", async () => {
		useAuthStatusMock.mockReturnValue({
			data: { initialized: false },
			isLoading: false,
			isError: false,
		});
		initMutateMock.mockImplementation((_values, options) => options.onSuccess());

		renderPage({ from: "/providers" });
		fireEvent.change(screen.getByLabelText("用户名"), { target: { value: "Admin" } });
		fireEvent.change(screen.getByLabelText("密码"), { target: { value: "Password" } });
		fireEvent.change(screen.getByLabelText("确认密码"), { target: { value: "Password" } });
		fireEvent.click(screen.getByRole("button", { name: "创建管理员" }));

		await waitFor(() => {
			expect(initMutateMock).toHaveBeenCalledWith(
				{ username: "Admin", password: "Password" },
				expect.anything(),
			);
		});
		await waitFor(() => {
			expect(screen.getByText("供应商页")).toBeInTheDocument();
		});
	});

	it("登录提交失败时提示错误", async () => {
		useAuthStatusMock.mockReturnValue({
			data: { initialized: true },
			isLoading: false,
			isError: false,
		});
		loginMutateMock.mockImplementation((_values, options) =>
			options.onError(new Error("用户名或密码错误")),
		);

		renderPage();
		fireEvent.change(screen.getByLabelText("用户名"), { target: { value: "Admin" } });
		fireEvent.change(screen.getByLabelText("密码"), { target: { value: "wrong" } });
		fireEvent.click(screen.getByRole("button", { name: "登录" }));

		await waitFor(() => {
			expect(toastError).toHaveBeenCalledWith("登录失败", expect.any(Error));
		});
	});

	it("已登录用户访问登录页直接回跳", () => {
		useMeMock.mockReturnValue({ data: { username: "Admin" }, isLoading: false, isError: false });
		useAuthStatusMock.mockReturnValue({
			data: { initialized: true },
			isLoading: false,
			isError: false,
		});

		renderPage();

		expect(screen.getByText("首页")).toBeInTheDocument();
	});
});
