import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

interface StatusBadgeProps {
	status: "enabled" | "disabled" | "success" | "error" | "warning";
	label?: string;
}

const variants: Record<StatusBadgeProps["status"], string> = {
	enabled: "bg-green-50 text-green-600 hover:bg-green-50 dark:bg-green-500/15 dark:text-green-400",
	disabled:
		"bg-slate-100/70 text-slate-500 hover:bg-slate-100/70 dark:bg-white/5 dark:text-slate-400",
	success: "bg-green-50 text-green-600 hover:bg-green-50 dark:bg-green-500/15 dark:text-green-400",
	error: "bg-red-50 text-red-500 hover:bg-red-50 dark:bg-red-500/15 dark:text-red-400",
	warning: "bg-amber-50 text-amber-600 hover:bg-amber-50 dark:bg-amber-500/15 dark:text-amber-400",
};

const defaultLabels: Record<StatusBadgeProps["status"], string> = {
	enabled: "启用",
	disabled: "禁用",
	success: "成功",
	error: "失败",
	warning: "警告",
};

export function StatusBadge({ status, label }: StatusBadgeProps) {
	return <Badge className={cn(variants[status])}>{label ?? defaultLabels[status]}</Badge>;
}
