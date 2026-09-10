import { authKeys, useLogin, useLogout } from "@/hooks/use-auth";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
	post: vi.fn(),
}));

vi.mock("@/lib/api", async () => {
	const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
	return {
		...actual,
		// ky 的调用形态是 api.post(path).json()——post 返回带 json 的对象。
		api: {
			post: (path: string) => ({ json: () => mocks.post(path) }),
			get: vi.fn(),
		},
	};
});

/** 业务层把 api.post(...).json() 的返回值直接交给 unwrap——返回后端信封即可。 */
function ok<T>(data: T) {
	return { code: "0", msg: "ok", data };
}

function wrapperWith(client: QueryClient) {
	return ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={client}>{children}</QueryClientProvider>
	);
}

describe("use-auth 直测（21-04）", () => {
	let queryClient: QueryClient;

	beforeEach(() => {
		mocks.post.mockReset();
		queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
	});

	it("useLogin 成功后双写 me 与 status 缓存（免额外重取）", async () => {
		mocks.post.mockImplementation(async () => ok({ username: "admin" }));
		const { result } = renderHook(() => useLogin(), { wrapper: wrapperWith(queryClient) });

		result.current.mutate({ username: "admin", password: "pw" });

		await waitFor(() => expect(result.current.isError || result.current.isSuccess).toBe(true));
		if (result.current.isError) throw result.current.error;
		expect(mocks.post).toHaveBeenCalledWith("auth/login");
		// 双写：守卫读 me 立即放行，登录页读 status 不再请求。
		expect(queryClient.getQueryData(authKeys.me)).toEqual({ username: "admin" });
		expect(queryClient.getQueryData(authKeys.status)).toEqual({ initialized: true });
	});

	it("useLogout 在 onSettled 清空 me（失败也收敛到登录页）", async () => {
		queryClient.setQueryData(authKeys.me, { username: "admin" });
		mocks.post.mockImplementation(() => Promise.reject(new Error("boom")));
		const { result } = renderHook(() => useLogout(), { wrapper: wrapperWith(queryClient) });

		result.current.mutate();

		await waitFor(() => expect(result.current.isError).toBe(true));
		// 失败路径同样清空会话数据，RequireAuth 据此跳登录页。
		expect(queryClient.getQueryData(authKeys.me)).toBeNull();
	});
});
