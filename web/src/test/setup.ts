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

if (typeof window !== "undefined" && typeof window.localStorage === "undefined") {
	Object.defineProperty(window, "localStorage", {
		value: new LocalStorageMock(),
		configurable: true,
		writable: true,
	});
}

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
