import { ProviderList, computeReorderIds } from "@/components/providers/ProviderList";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
	return {
		reorderMutate: vi.fn(),
	};
});

vi.mock("@/hooks/use-providers", async () => {
	const actual =
		await vi.importActual<typeof import("@/hooks/use-providers")>("@/hooks/use-providers");
	return {
		...actual,
		useReorderProviders: () => ({ mutate: mocks.reorderMutate }),
	};
});

interface ProviderFixture {
	id: number;
	name: string;
	enable: boolean;
	protocolType: number;
}

function makeProvider(overrides: Partial<ProviderFixture> = {}): ProviderFixture {
	return {
		id: 1,
		name: "DeepSeek",
		enable: true,
		protocolType: 0,
		...overrides,
	};
}

function renderList(providers: ProviderFixture[]) {
	const queryClient = new QueryClient();
	return render(
		<QueryClientProvider client={queryClient}>
			<ProviderList providers={providers as never} selectedId={null} onSelect={() => {}} />
		</QueryClientProvider>,
	);
}

describe("computeReorderIds（拖拽顺序计算）", () => {
	const items = [
		makeProvider({ id: 1, name: "Alpha" }),
		makeProvider({ id: 2, name: "Bravo" }),
		makeProvider({ id: 3, name: "Charlie" }),
	];

	it("把 activeId 移到 overId 所在位置", () => {
		// 把 Charlie 拖到第一位。
		expect(computeReorderIds(items, 3, 1)).toEqual([3, 1, 2]);
		// 把 Alpha 拖到末尾。
		expect(computeReorderIds(items, 1, 3)).toEqual([2, 3, 1]);
	});

	it("拖到原位置返回 null", () => {
		expect(computeReorderIds(items, 2, 2)).toBeNull();
	});

	it("任一 id 不存在返回 null", () => {
		expect(computeReorderIds(items, 99, 1)).toBeNull();
		expect(computeReorderIds(items, 1, 99)).toBeNull();
	});
});

describe("ProviderList 拖拽排序", () => {
	beforeEach(() => {
		mocks.reorderMutate.mockClear();
	});

	it("空列表展示空态", () => {
		renderList([]);
		expect(screen.getByText(/暂无 Provider/)).toBeTruthy();
	});

	it("每行渲染拖拽手柄", () => {
		renderList([makeProvider(), makeProvider({ id: 2, name: "Bravo" })]);
		expect(document.querySelectorAll("[aria-roledescription='sortable']")).toHaveLength(2);
	});

	it("点击行触发 onSelect", () => {
		const onSelect = vi.fn();
		const queryClient = new QueryClient();
		render(
			<QueryClientProvider client={queryClient}>
				<ProviderList providers={[makeProvider()] as never} selectedId={null} onSelect={onSelect} />
			</QueryClientProvider>,
		);
		screen.getByText("DeepSeek").click();
		expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 1, name: "DeepSeek" }));
	});
});
