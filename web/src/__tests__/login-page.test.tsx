import LoginPage from "@/pages/login";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

const saveInitSettingsMock = vi.fn(async () => ({ ok: true, failed: [] as string[] }));
vi.mock("@/hooks/use-init-settings", async (importOriginal) => ({
	...(await importOriginal<typeof import("@/hooks/use-init-settings")>()),
	saveInitSettings: (...args: unknown[]) => saveInitSettingsMock(...(args as [])),
}));

function renderPage(state?: { from?: string }) {
	const queryClient = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	return render(
		<QueryClientProvider client={queryClient}>
			<MemoryRouter initialEntries={[{ pathname: "/login", state }]}>
				<Routes>
					<Route path="/login" element={<LoginPage />} />
					<Route path="/" element={<div>首页</div>} />
					<Route path="/providers" element={<div>供应商页</div>} />
				</Routes>
			</MemoryRouter>
		</QueryClientProvider>,
	);
}

describe("LoginPage", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		saveInitSettingsMock.mockResolvedValue({ ok: true, failed: [] });
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

	it("初始化成功后（会话已建立）才写入引导时区与语言", async () => {
		useAuthStatusMock.mockReturnValue({
			data: { initialized: false },
			isLoading: false,
			isError: false,
		});
		// 模拟后端：init 请求在途期间会话尚未建立，设置写入必须等 onSuccess。
		let resolveInit: (() => void) | undefined;
		initMutateMock.mockImplementation((_values, options) => {
			resolveInit = () => options.onSuccess();
		});

		renderPage();
		fireEvent.change(screen.getByLabelText("用户名"), { target: { value: "Admin" } });
		fireEvent.change(screen.getByLabelText("密码"), { target: { value: "Password" } });
		fireEvent.change(screen.getByLabelText("确认密码"), { target: { value: "Password" } });
		fireEvent.click(screen.getByRole("button", { name: "创建管理员" }));
		await waitFor(() => expect(initMutateMock).toHaveBeenCalled());
		// 提交瞬间不得发设置写入（21-01：PUT 早于会话建立会 401 丢时区）。
		await Promise.resolve();
		expect(saveInitSettingsMock).not.toHaveBeenCalled();

		act(() => resolveInit?.());
		await waitFor(() => expect(saveInitSettingsMock).toHaveBeenCalledTimes(1));
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
