import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
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
