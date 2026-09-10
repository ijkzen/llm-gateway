import { useEffect, useState } from "react";

/**
 * 输入防抖值：`delayMs` 内的连续变更只产出最后一次（供逐键触发的查询去抖）。
 * 17-03（模板 URL 匹配）与 17-27（目录搜索）共用。
 */
export function useDebouncedValue<T>(value: T, delayMs: number): T {
	const [debounced, setDebounced] = useState(value);
	useEffect(() => {
		const timer = setTimeout(() => setDebounced(value), delayMs);
		return () => clearTimeout(timer);
	}, [value, delayMs]);
	return debounced;
}
