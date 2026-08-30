import { useEffect, useRef, useState } from "react";

interface UseInViewOptions {
	/** 元素进入视口后是否保持 true（默认 true，不再退出观察）。 */
	once?: boolean;
	/** 进入视口的判定阈值（0~1，元素可见比例）。 */
	threshold?: number;
	/** 根元素（默认浏览器视口）。 */
	rootMargin?: string;
}

/**
 * 懒加载观察 hook：元素进入视口后置 `inView = true`。
 *
 * 用于赛马卡片等「滚动到才发请求」的场景——配合 TanStack Query 的
 * `enabled` 条件使用。jsdom 等无 IntersectionObserver 的环境兜底为
 * 立即进入视口（行为同非懒加载，避免测试阻塞）。
 */
export function useInView({
	once = true,
	threshold = 0,
	rootMargin = "0px",
}: UseInViewOptions = {}) {
	const ref = useRef<HTMLDivElement | null>(null);
	const [inView, setInView] = useState(false);

	useEffect(() => {
		const element = ref.current;
		if (!element) {
			return;
		}
		if (typeof IntersectionObserver === "undefined") {
			// 无 IO 环境（jsdom）直接视为可见。
			setInView(true);
			return;
		}

		const observer = new IntersectionObserver(
			(entries) => {
				for (const entry of entries) {
					if (entry.isIntersecting) {
						setInView(true);
						if (once) {
							observer.disconnect();
						}
					} else if (!once) {
						setInView(false);
					}
				}
			},
			{ threshold, rootMargin },
		);
		observer.observe(element);
		return () => observer.disconnect();
	}, [once, threshold, rootMargin]);

	return { ref, inView };
}
