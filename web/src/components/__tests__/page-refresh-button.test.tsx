import { PageRefreshButton } from "@/components/page-refresh-button";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

/** 探针组件：模拟「当前页面挂载的查询」，取数函数由用例注入以控制时序。 */
function Probe({ queryFn }: { queryFn: () => Promise<string> }) {
	const { data } = useQuery({ queryKey: ["probe"], queryFn });
	return <div data-testid="probe">{data ?? "pending"}</div>;
}

function makeClient() {
	return new QueryClient({
		defaultOptions: { queries: { retry: false, staleTime: 5 * 60 * 1000 } },
	});
}

function renderButton(client: QueryClient, probe?: React.ReactNode) {
	return render(
		<QueryClientProvider client={client}>
			<PageRefreshButton />
			{probe}
		</QueryClientProvider>,
	);
}

function refreshButton() {
	return screen.getByRole("button", { name: "刷新" });
}

describe("PageRefreshButton 页面刷新语义", () => {
	it("清空非全局的 inactive 缓存数据", async () => {
		const client = makeClient();
		client.setQueryData(["providers"], [{ id: 1 }]);
		client.setQueryData(["stats", "summary", null, null], { totalRequests: 1 });
		client.setQueryData(["auth", "me"], { username: "admin" });
		client.setQueryData(["health"], { version: "0.1.18" });

		renderButton(client);
		fireEvent.click(refreshButton());

		await waitFor(() => {
			expect(client.getQueryData(["providers"])).toBeUndefined();
		});
		expect(client.getQueryData(["stats", "summary", null, null])).toBeUndefined();
		expect(client.getQueryData(["auth", "me"])).toEqual({ username: "admin" });
		expect(client.getQueryData(["health"])).toEqual({ version: "0.1.18" });
	});

	it("保留 auth/me 与 health 数据且不重新取数", async () => {
		const client = makeClient();
		const meFn = vi.fn(async () => ({ username: "admin" }));
		const healthFn = vi.fn(async () => ({ version: "0.1.18" }));
		const probeFn = vi.fn(async () => "probe-data");
		// 布局级与普通查询都挂载为 active：predicate 漏掉排除则布局级被重取（meFn 再被调用），
		// predicate 过宽（连普通查询也排除）则探针计数不增，两条断言合起来钉住排除范围。
		function ProbeWithLayout() {
			useQuery({ queryKey: ["auth", "me"], queryFn: meFn });
			useQuery({ queryKey: ["health"], queryFn: healthFn });
			return <Probe queryFn={probeFn} />;
		}

		render(
			<QueryClientProvider client={client}>
				<PageRefreshButton />
				<ProbeWithLayout />
			</QueryClientProvider>,
		);
		await waitFor(() => {
			expect(screen.getByTestId("probe").textContent).toBe("probe-data");
		});
		meFn.mockClear();
		healthFn.mockClear();

		fireEvent.click(refreshButton());
		// 刷新确实执行过：普通查询被重取。
		await waitFor(() => {
			expect(probeFn).toHaveBeenCalledTimes(2);
		});

		expect(meFn).not.toHaveBeenCalled();
		expect(healthFn).not.toHaveBeenCalled();
		expect(client.getQueryData(["auth", "me"])).toEqual({ username: "admin" });
		expect(client.getQueryData(["health"])).toEqual({ version: "0.1.18" });
	});

	it("重新取数当前页挂载的查询（忽略 staleTime）", async () => {
		const client = makeClient();
		const probeFn = vi.fn(async () => "fresh");
		// 预置一份「5 分钟内仍然新鲜」的缓存：若刷新不忽略 staleTime 就不会重取。
		client.setQueryData(["probe"], "stale-old");

		renderButton(client, <Probe queryFn={probeFn} />);
		expect(screen.getByTestId("probe").textContent).toBe("stale-old");

		fireEvent.click(refreshButton());

		await waitFor(() => {
			expect(screen.getByTestId("probe").textContent).toBe("fresh");
		});
		expect(probeFn).toHaveBeenCalledTimes(1);
	});

	it("重取期间按钮禁用且图标旋转，完成后恢复", async () => {
		const client = makeClient();
		let resolveFetch: (() => void) | undefined;
		const probeFn = () =>
			new Promise<string>((resolve) => {
				resolveFetch = () => resolve("new");
			});
		client.setQueryData(["probe"], "old");

		renderButton(client, <Probe queryFn={probeFn} />);

		expect(refreshButton()).not.toBeDisabled();
		fireEvent.click(refreshButton());

		await waitFor(() => {
			expect(refreshButton()).toBeDisabled();
		});
		expect(refreshButton().querySelector(".animate-spin")).not.toBeNull();

		resolveFetch?.();
		await waitFor(() => {
			expect(refreshButton()).not.toBeDisabled();
		});
		expect(refreshButton().querySelector(".animate-spin")).toBeNull();
		expect(screen.getByTestId("probe").textContent).toBe("new");
	});
});
