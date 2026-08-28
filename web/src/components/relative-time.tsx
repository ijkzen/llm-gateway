import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useEffect, useState } from "react";

interface RelativeTimeProps {
	date: string | Date;
	fallback?: string;
}

function formatRelativeTime(date: Date): string {
	const now = new Date();
	const diffMs = now.getTime() - date.getTime();
	const diffSec = Math.floor(diffMs / 1000);
	if (diffSec < 60) return "刚刚";
	const diffMin = Math.floor(diffSec / 60);
	if (diffMin < 60) return `${diffMin} 分钟前`;
	const diffHour = Math.floor(diffMin / 60);
	if (diffHour < 24) return `${diffHour} 小时前`;
	const diffDay = Math.floor(diffHour / 24);
	if (diffDay < 30) return `${diffDay} 天前`;
	return date.toLocaleDateString("zh-CN");
}

export function RelativeTime({ date, fallback = "—" }: RelativeTimeProps) {
	const parsed = typeof date === "string" ? new Date(date) : date;
	const ts = parsed.getTime();
	const isValid = !Number.isNaN(ts) && ts > 0;

	const [, setTick] = useState(0);

	useEffect(() => {
		const id = setInterval(() => setTick((t) => t + 1), 60_000);
		return () => clearInterval(id);
	}, []);

	if (!isValid) {
		return <span className="text-muted-foreground">{fallback}</span>;
	}

	const full = parsed.toLocaleString("zh-CN");
	const relative = formatRelativeTime(parsed);

	return (
		<Tooltip>
			<TooltipTrigger asChild>
				<span className="cursor-help">{relative}</span>
			</TooltipTrigger>
			<TooltipContent>{full}</TooltipContent>
		</Tooltip>
	);
}
