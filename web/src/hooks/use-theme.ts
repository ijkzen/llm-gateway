import { useEffect } from "react";
import { create } from "zustand";
import { persist } from "zustand/middleware";

export type Theme = "light" | "dark" | "system";
export type ResolvedTheme = Exclude<Theme, "system">;

function resolveTheme(theme: Theme): ResolvedTheme {
	if (theme === "system") {
		return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
	}
	return theme;
}

function applyResolvedTheme(resolved: ResolvedTheme) {
	document.documentElement.classList.toggle("dark", resolved === "dark");
	// 同步 meta color-scheme，让浏览器原生 UI（滚动条、表单控件）跟随主题
	document.querySelector('meta[name="color-scheme"]')?.setAttribute("content", resolved);
}

// Capture icon center for circular reveal animation.
// Inject a dynamic <style> tag to bake coordinates directly
// into @keyframes, avoiding CSS custom-property inheritance
// issues across the ::view-transition pseudo-element tree.
function injectViewTransitionStyle(buttonRef?: React.RefObject<HTMLElement | null>) {
	let cx = window.innerWidth / 2;
	let cy = window.innerHeight / 2;
	if (buttonRef?.current) {
		const icon = buttonRef.current.querySelector("svg");
		const el = icon ?? buttonRef.current;
		const rect = el.getBoundingClientRect();
		cx = rect.left + rect.width / 2;
		cy = rect.top + rect.height / 2;
	}

	const styleId = "hermes-theme-vt-style";
	// Remove any previous injected style
	document.getElementById(styleId)?.remove();

	const style = document.createElement("style");
	style.id = styleId;
	style.textContent = [
		// Old snapshot fades out over the same duration as the
		// circle reveal, so there is no blank flash between them.
		// `both` (fill-mode) is required: without it the pseudo-element
		// styles revert when the animation ends (old snapshot jumps back
		// to opacity 1), painting one full frame of the previous theme
		// before the view-transition tree is torn down.
		"::view-transition-old(root) {",
		"  animation: vt-fade-out 0.4s ease-out both;",
		"}",
		"::view-transition-new(root) {",
		"  animation: reveal-circle 0.4s ease-out both;",
		"}",
		"@keyframes vt-fade-out {",
		"  to { opacity: 0; }",
		"}",
		"@keyframes reveal-circle {",
		`  from { clip-path: circle(0% at ${cx}px ${cy}px); }`,
		`  to   { clip-path: circle(150% at ${cx}px ${cy}px); }`,
		"}",
	].join("\n");
	document.head.appendChild(style);
	return style;
}

interface ThemeStore {
	theme: Theme;
	_hasHydrated: boolean;
	setHasHydrated: (v: boolean) => void;
	setTheme: (theme: Theme, buttonRef?: React.RefObject<HTMLElement | null>) => void;
}

export const useTheme = create<ThemeStore>()(
	persist(
		(set, get) => ({
			theme: "light",
			_hasHydrated: false,
			setHasHydrated: (hasHydrated) => set({ _hasHydrated: hasHydrated }),
			setTheme: (newTheme, buttonRef) => {
				if (newTheme === get().theme) return;

				const applyTheme = () => {
					set({ theme: newTheme });
					applyResolvedTheme(resolveTheme(newTheme));
				};

				// 用户偏好减少动态效果时跳过圆形揭示动画，直接切换
				const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
				if (!document.startViewTransition || prefersReducedMotion) {
					applyTheme();
					return;
				}

				const style = injectViewTransitionStyle(buttonRef);
				const vt = document.startViewTransition(applyTheme);
				// Clean up the injected style whether the transition
				// finishes normally or is skipped.
				vt.finished.finally(() => style.remove());
			},
		}),
		{
			name: "llm-gateway-theme",
			onRehydrateStorage: () => (state) => {
				state?.setHasHydrated(true);
			},
		},
	),
);

export function useInitTheme() {
	const theme = useTheme((state) => state.theme);
	const hasHydrated = useTheme((state) => state._hasHydrated);

	useEffect(() => {
		if (!hasHydrated) return;

		applyResolvedTheme(resolveTheme(theme));

		// 跟随系统时监听系统主题变化，其他模式无需监听
		if (theme !== "system") return;
		const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
		const handleChange = () => applyResolvedTheme(resolveTheme("system"));
		mediaQuery.addEventListener("change", handleChange);
		return () => mediaQuery.removeEventListener("change", handleChange);
	}, [theme, hasHydrated]);
}
