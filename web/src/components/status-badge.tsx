import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

interface StatusBadgeProps {
	status: "enabled" | "disabled" | "success" | "error" | "warning";
	label?: string;
}

const variants: Record<StatusBadgeProps["status"], string> = {
	enabled: "bg-success/10 text-success hover:bg-success/10",
	disabled: "bg-muted text-muted-foreground hover:bg-muted",
	success: "bg-success/10 text-success hover:bg-success/10",
	error: "bg-destructive/10 text-destructive hover:bg-destructive/10",
	warning: "bg-warning/10 text-warning hover:bg-warning/10",
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
