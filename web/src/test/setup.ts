import { vi } from "vitest";
import "@testing-library/jest-dom/vitest";

// Node 26 提供了实验性的全局 localStorage（未传 --localstorage-file 时为 undefined），
// 导致 Vitest jsdom 环境跳过注入 jsdom 自己的 localStorage，这里用一个内存实现补齐
class LocalStorageMock {
	private store = new Map<string, string>();

	get length() {
		return this.store.size;
	}

	clear() {
		this.store.clear();
	}

	getItem(key: string) {
		return this.store.get(key) ?? null;
	}

	setItem(key: string, value: string) {
		this.store.set(key, String(value));
	}

	removeItem(key: string) {
		this.store.delete(key);
	}

	key(index: number) {
		return [...this.store.keys()][index] ?? null;
	}
}

// 强制覆盖：Node 26 的 window.localStorage accessor 在未传
// --localstorage-file 时返回 undefined，直接 defineProperty 替换。
if (typeof window !== "undefined") {
	Object.defineProperty(window, "localStorage", {
		value: new LocalStorageMock(),
		configurable: true,
		writable: true,
	});
}

// 测试环境固定使用中文：现有断言均按 zh-CN 文案编写。
// jsdom 的 navigator.language 为 en-US，先写入语言偏好再动态加载 i18n
//（静态 import 会被提升到顶部先执行，读到 en-US 兜底）。
window.localStorage.setItem("llm-gateway-locale", JSON.stringify({ state: { locale: "zh-CN" } }));
await import("@/i18n");

// jsdom 未实现 ResizeObserver，Radix 弹窗类组件（Dialog/Checkbox 等）渲染时需要。
class ResizeObserverMock {
	observe() {}
	unobserve() {}
	disconnect() {}
}

if (typeof window !== "undefined" && typeof window.ResizeObserver === "undefined") {
	window.ResizeObserver = ResizeObserverMock as unknown as typeof ResizeObserver;
}

// jsdom 未实现 Element.prototype.scrollIntoView，组件里调用会抛
// "scrollIntoView is not a function"，这里补一个 no-op 桩。
if (typeof Element !== "undefined" && !Element.prototype.scrollIntoView) {
	Element.prototype.scrollIntoView = () => {};
}

// jsdom 未实现 window.matchMedia（use-mobile / 响应式组件用），补最小可用桩：
// 默认不匹配（桌面语义），监听器为空实现。
if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
	window.matchMedia = ((query: string) => ({
		matches: false,
		media: query,
		onchange: null,
		addEventListener: () => {},
		removeEventListener: () => {},
		addListener: () => {},
		removeListener: () => {},
		dispatchEvent: () => false,
	})) as unknown as typeof window.matchMedia;
}

// 数据面板口径时区 hook 全局 mock：测试里返回运行机器本地 IANA 时区，
// 使周期窗口推导退化为「浏览器本地」旧语义（tz 数学本身由
// race-period.test.ts 的固定 IANA 用例覆盖）。真实实现的直测见
// hooks/__tests__/use-stats-time-zone.test.tsx（19-22）。
vi.mock("@/hooks/use-stats-time-zone", () => ({
	useStatsTimeZone: () => Intl.DateTimeFormat().resolvedOptions().timeZone,
	DEFAULT_STATS_TIME_ZONE: "Asia/Shanghai",
	STATS_TIME_ZONE_KEY: "timezone",
}));
