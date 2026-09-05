import { MidEllipsis } from "@/components/mid-ellipsis";
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

const CHAR_WIDTH = 10;

const originalClientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
const originalOffsetWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetWidth");

afterEach(() => {
	if (originalClientWidth) {
		Object.defineProperty(HTMLElement.prototype, "clientWidth", originalClientWidth);
	}
	if (originalOffsetWidth) {
		Object.defineProperty(HTMLElement.prototype, "offsetWidth", originalOffsetWidth);
	}
});

// jsdom 无真实布局：按「每字符固定宽」mock 元素度量，驱动组件的二分切片逻辑
function mockMetrics(clientWidth: number) {
	Object.defineProperty(HTMLElement.prototype, "clientWidth", {
		configurable: true,
		get: () => clientWidth,
	});
	Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
		configurable: true,
		get(this: HTMLElement) {
			return (this.textContent?.length ?? 0) * CHAR_WIDTH;
		},
	});
}

describe("MidEllipsis", () => {
	it("宽度足够时原样渲染全文", () => {
		mockMetrics(300);
		render(<MidEllipsis text="short-text" />);
		expect(screen.getByText("short-text")).toBeInTheDocument();
	});

	it("超宽时保留首尾、中间收起为 …", () => {
		mockMetrics(100);
		render(<MidEllipsis text="abcdefghijklmnopqrst" />);
		// 20 字符宽 200px，容器 100px：可保留 9 字符（头 5 尾 4），加省略号恰 100px。
		// 溢出时隐藏测量节点持有同样切片文本，故经唯一 title 定位根元素断言。
		const el = screen.getByTitle("abcdefghijklmnopqrst");
		expect(el.textContent).toBe("abcde…qrst");
	});

	it("极窄时仅显示省略号", () => {
		mockMetrics(5);
		render(<MidEllipsis text="abcdefghijklmnopqrst" />);
		expect(screen.getByTitle("abcdefghijklmnopqrst").textContent).toBe("…");
	});

	it("title 提示始终为全文", () => {
		mockMetrics(100);
		render(<MidEllipsis text="abcdefghijklmnopqrst" />);
		expect(screen.getByTitle("abcdefghijklmnopqrst")).toHaveAttribute(
			"title",
			"abcdefghijklmnopqrst",
		);
	});
});
