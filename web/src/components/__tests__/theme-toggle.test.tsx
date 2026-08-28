import { ThemeToggle } from "@/components/theme-toggle";
import { useTheme } from "@/hooks/use-theme";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

function mockMatchMedia(matches: boolean) {
	window.matchMedia = vi.fn().mockImplementation((query: string) => ({
		matches,
		media: query,
		onchange: null,
		addListener: vi.fn(),
		removeListener: vi.fn(),
		addEventListener: vi.fn(),
		removeEventListener: vi.fn(),
		dispatchEvent: vi.fn(),
	}));
}

describe("ThemeToggle", () => {
	beforeEach(() => {
		window.localStorage.clear();
		useTheme.setState({ theme: "light", _hasHydrated: true });
		document.documentElement.classList.remove("dark");
		mockMatchMedia(false);
	});

	it("switches to dark theme from the dropdown", () => {
		render(<ThemeToggle />);

		fireEvent.keyDown(screen.getByRole("button", { name: "切换主题" }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitem", { name: /暗色/ }));

		expect(useTheme.getState().theme).toBe("dark");
		expect(document.documentElement.classList.contains("dark")).toBe(true);
	});

	it("resolves system theme via prefers-color-scheme", () => {
		mockMatchMedia(true);
		render(<ThemeToggle />);

		fireEvent.keyDown(screen.getByRole("button", { name: "切换主题" }), { key: "ArrowDown" });
		fireEvent.click(screen.getByRole("menuitem", { name: /跟随系统/ }));

		expect(useTheme.getState().theme).toBe("system");
		expect(document.documentElement.classList.contains("dark")).toBe(true);
	});
});
