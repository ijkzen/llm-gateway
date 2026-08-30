import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

function trimTrailingZeros(text: string): string {
	return text.replace(/\.?0+$/, "");
}

/** 超过 maxLength 时按码点保留首尾、中间以 … 省略（如 "阿里云・deepseek-chat" → "阿里云…eepseek-chat"）。 */
export function middleEllipsis(text: string, maxLength: number): string {
	if (text.length <= maxLength) {
		return text;
	}
	const chars = Array.from(text);
	const ellipsis = "…";
	const keep = Math.max(0, maxLength - ellipsis.length);
	const head = Math.ceil(keep / 2);
	const tail = Math.floor(keep / 2);
	return `${chars.slice(0, head).join("")}${ellipsis}${chars.slice(-tail).join("")}`;
}

/** token 数量按中文计数习惯缩写：>= 1 亿用「亿」（两位小数），>= 1 万用「万」（一位小数）。 */
export function formatTokenCount(value: number): string {
	if (value >= 100_000_000) {
		return `${trimTrailingZeros((value / 100_000_000).toFixed(2))} 亿`;
	}
	if (value >= 10_000) {
		return `${trimTrailingZeros((value / 10_000).toFixed(1))} 万`;
	}
	return value.toLocaleString("zh-CN");
}

/**
 * 比率（0~1）转百分比展示：只做 ×100 单位换算，不改变后端给定的精度
 * （后端各接口已按 5 位小数完备处理，如 0.13333 → "13.333%"）。
 * toFixed(5) 仅消除 IEEE754 乘法噪声（0.29*100 → 28.999…），不截断精度。
 */
export function formatPercent(rate: number): string {
	return `${Number((rate * 100).toFixed(5))}%`;
}

/**
 * 模型上下文长度按 1000 进制缩写（业界惯例，128K = 128,000 tokens）：
 * 128000 → 128K、131072 → 131.1K、1000000 → 1M；小于 1000 原样返回。
 */
export function formatContextLength(value: number): string {
	if (value >= 1_000_000) {
		return `${trimTrailingZeros((value / 1_000_000).toFixed(1))}M`;
	}
	if (value >= 1_000) {
		return `${trimTrailingZeros((value / 1_000).toFixed(1))}K`;
	}
	return String(value);
}

/** 按 value 降序取前 limit 名，其余合并为 other（value 求和）。 */
export function topWithOther<T extends { value: number }>(items: T[], other: T, limit = 10): T[] {
	const sorted = [...items].sort((a, b) => b.value - a.value);
	if (sorted.length <= limit) {
		return sorted;
	}
	const restValue = sorted.slice(limit).reduce((sum, item) => sum + item.value, 0);
	return [...sorted.slice(0, limit), { ...other, value: restValue }];
}

// 计算分页页码序列，最多展示 5 个页码，超出部分用 "..." 折叠
export function getPageNumbers(currentPage: number, totalPages: number): (number | "...")[] {
	const maxVisiblePages = 5;
	const rangeWithDots: (number | "...")[] = [];

	if (totalPages <= maxVisiblePages) {
		for (let i = 1; i <= totalPages; i++) {
			rangeWithDots.push(i);
		}
	} else if (currentPage <= 3) {
		// 靠近开头：[1] [2] [3] [4] ... [10]
		for (let i = 1; i <= 4; i++) {
			rangeWithDots.push(i);
		}
		rangeWithDots.push("...", totalPages);
	} else if (currentPage >= totalPages - 2) {
		// 靠近结尾：[1] ... [7] [8] [9] [10]
		rangeWithDots.push(1, "...");
		for (let i = totalPages - 3; i <= totalPages; i++) {
			rangeWithDots.push(i);
		}
	} else {
		// 中间：[1] ... [4] [5] [6] ... [10]
		rangeWithDots.push(1, "...");
		for (let i = currentPage - 1; i <= currentPage + 1; i++) {
			rangeWithDots.push(i);
		}
		rangeWithDots.push("...", totalPages);
	}

	return rangeWithDots;
}
