import { useLayoutEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

type MidEllipsisProps = {
	text: string;
	className?: string;
	title?: string;
};

/**
 * 单行中间省略：超宽时按码点保留首尾、中间收起为 …；不超宽时原样展示。
 * 溢出宽度由测量元素实测（二分最大可保留字符数），容器尺寸变化经 ResizeObserver 重算。
 */
export function MidEllipsis({ text, className, title }: MidEllipsisProps) {
	const boxRef = useRef<HTMLSpanElement>(null);
	const measureRef = useRef<HTMLSpanElement>(null);
	const [sliced, setSliced] = useState<string | null>(null);

	useLayoutEffect(() => {
		const box = boxRef.current;
		const measure = measureRef.current;
		if (!box || !measure) return;

		const recompute = () => {
			const available = box.clientWidth;
			measure.textContent = text;
			if (measure.offsetWidth <= available) {
				// 未溢出时清空测量文本，避免隐藏节点携带与展示相同的字符串
				measure.textContent = "";
				setSliced(null);
				return;
			}
			const chars = Array.from(text);
			const build = (keep: number) => {
				const head = Math.ceil(keep / 2);
				const tail = Math.floor(keep / 2);
				const tailPart = tail > 0 ? chars.slice(-tail).join("") : "";
				return `${chars.slice(0, head).join("")}…${tailPart}`;
			};
			let lo = 0;
			let hi = chars.length - 1;
			let best = 0;
			while (lo <= hi) {
				const mid = Math.floor((lo + hi) / 2);
				measure.textContent = build(mid);
				if (measure.offsetWidth <= available) {
					best = mid;
					lo = mid + 1;
				} else {
					hi = mid - 1;
				}
			}
			// 稳态下清空测量文本：切片已由 state 渲染，隐藏节点不保留任何字符串
			measure.textContent = "";
			setSliced(best > 0 ? build(best) : "…");
		};

		recompute();
		const observer = new ResizeObserver(recompute);
		observer.observe(box);
		return () => observer.disconnect();
	}, [text]);

	return (
		<span
			ref={boxRef}
			className={cn("block max-w-full overflow-hidden whitespace-nowrap", className)}
			title={title ?? text}
		>
			{sliced ?? text}
			<span ref={measureRef} aria-hidden="true" className="invisible absolute whitespace-nowrap" />
		</span>
	);
}
