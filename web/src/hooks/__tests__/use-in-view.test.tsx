import { useInView } from "@/hooks/use-in-view";
import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

/**
 * 可手动触发的 IntersectionObserver mock：observer.observe 记录元素，
 * 测试用触发回调模拟元素进入/离开视口。
 */
class MockIntersectionObserver {
	static instances: MockIntersectionObserver[] = [];
	readonly elements: Element[] = [];
	private callback: IntersectionObserverCallback;

	constructor(callback: IntersectionObserverCallback) {
		this.callback = callback;
		MockIntersectionObserver.instances.push(this);
	}

	observe(element: Element) {
		this.elements.push(element);
	}

	unobserve() {}

	disconnect() {}

	/** 模拟元素进入视口。 */
	intersect(intersecting: boolean) {
		this.callback(
			this.elements.map((target) => ({
				target,
				isIntersecting: intersecting,
			})) as IntersectionObserverEntry[],
			this as unknown as IntersectionObserver,
		);
	}

	takeRecords() {
		return [];
	}

	static reset() {
		MockIntersectionObserver.instances = [];
	}
}

/** 取最近一次实例（测试前置已断言存在）。 */
function instance(): MockIntersectionObserver {
	const first = MockIntersectionObserver.instances[0];
	if (!first) {
		throw new Error("IntersectionObserver 尚未实例化");
	}
	return first;
}

describe("useInView", () => {
	afterEach(() => {
		MockIntersectionObserver.reset();
		vi.unstubAllGlobals();
	});

	function Harness({ once = true }: { once?: boolean }) {
		const { ref, inView } = useInView({ once });
		return (
			<div ref={ref} data-testid="target">
				{inView ? "visible" : "hidden"}
			</div>
		);
	}

	it("初始为不可见，进入视口后变为可见", () => {
		vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
		render(<Harness />);

		expect(screen.getByTestId("target").textContent).toBe("hidden");

		expect(MockIntersectionObserver.instances).toHaveLength(1);
		const observer = instance();
		act(() => observer.intersect(true));
		expect(screen.getByTestId("target").textContent).toBe("visible");
	});

	it("once=true 时离开视口不再变回不可见", () => {
		vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
		render(<Harness once />);

		const observer = instance();
		act(() => observer.intersect(true));
		act(() => observer.intersect(false));
		expect(screen.getByTestId("target").textContent).toBe("visible");
	});

	it("无 IntersectionObserver 环境（jsdom）直接视为可见", () => {
		render(<Harness />);
		expect(screen.getByTestId("target").textContent).toBe("visible");
	});
});
