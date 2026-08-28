import { useInitTheme, useTheme } from "@/hooks/use-theme";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

describe("useInitTheme", () => {
	beforeEach(() => {
		window.localStorage.clear();
		document.documentElement.classList.remove("dark");
	});

	it("applies the dark class for the dark theme", () => {
		useTheme.setState({ theme: "dark", _hasHydrated: true });
		renderHook(() => useInitTheme());

		expect(document.documentElement.classList.contains("dark")).toBe(true);
	});

	it("follows system color scheme changes when theme is system", () => {
		let darkMatches = false;
		let changeHandler: (() => void) | null = null;
		window.matchMedia = vi.fn().mockImplementation((query: string) => ({
			get matches() {
				return darkMatches;
			},
			media: query,
			onchange: null,
			addListener: vi.fn(),
			removeListener: vi.fn(),
			addEventListener: (_: string, cb: () => void) => {
				changeHandler = cb;
			},
			removeEventListener: vi.fn(),
			dispatchEvent: vi.fn(),
		}));

		useTheme.setState({ theme: "system", _hasHydrated: true });
		renderHook(() => useInitTheme());

		expect(document.documentElement.classList.contains("dark")).toBe(false);

		darkMatches = true;
		act(() => changeHandler?.());
		expect(document.documentElement.classList.contains("dark")).toBe(true);
	});
});
