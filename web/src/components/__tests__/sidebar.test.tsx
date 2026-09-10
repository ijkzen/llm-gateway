import { Sidebar, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

function renderSidebar() {
	return render(
		<SidebarProvider>
			<Sidebar>
				<div>导航内容</div>
			</Sidebar>
			<SidebarTrigger />
		</SidebarProvider>,
	);
}

/** 桌面侧栏根节点的 data-state（expanded / collapsed）。 */
function sidebarState(): string | null {
	return document.querySelector("[data-state][data-variant]")?.getAttribute("data-state") ?? null;
}

describe("SidebarProvider 折叠与快捷键（19-01 / 19-09）", () => {
	beforeEach(() => {
		document.cookie = "sidebar_state=; path=/; max-age=0";
	});

	it("默认展开并渲染内容与触发按钮", () => {
		renderSidebar();
		expect(sidebarState()).toBe("expanded");
		expect(screen.getByText("导航内容")).toBeTruthy();
		expect(screen.getByRole("button", { name: /切换|Toggle/ })).toBeTruthy();
	});

	it("点击触发按钮折叠并写入 cookie（19-01：刷新可恢复）", () => {
		renderSidebar();
		fireEvent.click(screen.getByRole("button", { name: /切换|Toggle/ }));

		expect(sidebarState()).toBe("collapsed");
		expect(document.cookie).toContain("sidebar_state=false");
	});

	it("cookie 为 false 时初始渲染即折叠（19-01 读取侧）", () => {
		document.cookie = "sidebar_state=false; path=/";
		renderSidebar();
		expect(sidebarState()).toBe("collapsed");
	});

	it("输入框内按 Ctrl+B 不切换侧栏（19-09）", () => {
		render(
			<SidebarProvider>
				<Sidebar>
					<div>导航内容</div>
				</Sidebar>
				<input aria-label="文本框" />
			</SidebarProvider>,
		);
		fireEvent.keyDown(screen.getByLabelText("文本框"), { key: "b", ctrlKey: true, bubbles: true });
		expect(sidebarState()).toBe("expanded");
	});

	it("长按产生的重复按键被忽略（19-09）", () => {
		renderSidebar();
		fireEvent.keyDown(window, { key: "b", ctrlKey: true, repeat: true });
		expect(sidebarState()).toBe("expanded");
	});

	it("非编辑目标按 Ctrl+B 正常切换", () => {
		renderSidebar();
		fireEvent.keyDown(window, { key: "b", ctrlKey: true });
		expect(sidebarState()).toBe("collapsed");
	});
});
