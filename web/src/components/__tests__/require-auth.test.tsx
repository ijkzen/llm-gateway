import { RequireAuth } from "@/components/require-auth";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const useMeMock = vi.fn();

vi.mock("@/hooks/use-auth", () => ({
	useMe: () => useMeMock(),
}));

function renderAt(path: string) {
	return render(
		<MemoryRouter initialEntries={[path]}>
			<Routes>
				<Route
					path="/"
					element={
						<RequireAuth>
							<div>受保护内容</div>
						</RequireAuth>
					}
				/>
				<Route path="/login" element={<div>登录页</div>} />
			</Routes>
		</MemoryRouter>,
	);
}

describe("RequireAuth", () => {
	beforeEach(() => {
		useMeMock.mockReset();
	});

	it("加载中显示验证状态提示", () => {
		useMeMock.mockReturnValue({ data: undefined, isLoading: true, isError: false });

		renderAt("/");

		expect(screen.getByText("正在验证登录状态...")).toBeInTheDocument();
	});

	it("未登录跳转到登录页", () => {
		useMeMock.mockReturnValue({ data: undefined, isLoading: false, isError: true });

		renderAt("/");

		expect(screen.getByText("登录页")).toBeInTheDocument();
	});

	it("已登录渲染受保护内容", () => {
		useMeMock.mockReturnValue({
			data: { username: "Admin" },
			isLoading: false,
			isError: false,
		});

		renderAt("/");

		expect(screen.getByText("受保护内容")).toBeInTheDocument();
	});
});
