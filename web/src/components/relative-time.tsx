import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

interface RelativeTimeProps {
	date: string | Date;
	fallback?: string;
}

export function RelativeTime({ date, fallback = "—" }: RelativeTimeProps) {
	const { t, i18n } = useTranslation();
	const parsed = typeof date === "string" ? new Date(date) : date;
	const ts = parsed.getTime();
	const isValid = !Number.isNaN(ts) && ts > 0;

	const [, setTick] = useState(0);

	useEffect(() => {
		const id = setInterval(() => setTick((tick) => tick + 1), 60_000);
		return () => clearInterval(id);
	}, []);

	if (!isValid) {
		return <span className="text-muted-foreground">{fallback}</span>;
	}

	const locale = i18n.language.startsWith("zh") ? "zh-CN" : "en-US";
	const full = parsed.toLocaleString(locale);
	const relative = formatRelativeTime(parsed, locale, t);

	return (
		<Tooltip>
			<TooltipTrigger asChild>
				<span className="cursor-help">{relative}</span>
			</TooltipTrigger>
			<TooltipContent>{full}</TooltipContent>
		</Tooltip>
	);
}

function formatRelativeTime(
	date: Date,
	locale: string,
	t: (key: string, opts?: Record<string, unknown>) => string,
): string {
	const now = new Date();
	const diffMs = now.getTime() - date.getTime();
	const diffSec = Math.floor(diffMs / 1000);
	if (diffSec < 60) return t("relativeTime.justNow");
	const diffMin = Math.floor(diffSec / 60);
	if (diffMin < 60) return t("relativeTime.minutesAgo", { count: diffMin });
	const diffHour = Math.floor(diffMin / 60);
	if (diffHour < 24) return t("relativeTime.hoursAgo", { count: diffHour });
	const diffDay = Math.floor(diffHour / 24);
	if (diffDay < 30) return t("relativeTime.daysAgo", { count: diffDay });
	return date.toLocaleDateString(locale);
}
