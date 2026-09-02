import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";

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

const defaultLabelKeys: Record<StatusBadgeProps["status"], string> = {
	enabled: "providers.enable",
	disabled: "common.disabled",
	success: "cronJobs.status.success",
	error: "cronJobs.status.failed",
	warning: "common.unknown",
};

export function StatusBadge({ status, label }: StatusBadgeProps) {
	const { t } = useTranslation();
	return <Badge className={cn(variants[status])}>{label ?? t(defaultLabelKeys[status])}</Badge>;
}
