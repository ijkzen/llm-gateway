import { Card, CardContent } from "@/components/ui/card";
import type { LucideIcon } from "lucide-react";

interface StatsCardProps {
	icon: LucideIcon;
	label: string;
	value: React.ReactNode;
	subLabel?: string;
}

export function StatsCard({ icon: Icon, label, value, subLabel }: StatsCardProps) {
	return (
		<Card>
			<CardContent className="flex items-start gap-4 p-6">
				<div className="flex size-10 items-center justify-center rounded-xl bg-primary/10 text-primary">
					<Icon className="size-5" />
				</div>
				<div className="min-w-0">
					<p className="text-sm font-medium text-muted-foreground">{label}</p>
					<p className="text-2xl font-bold">{value}</p>
					{subLabel && <p className="text-xs text-muted-foreground">{subLabel}</p>}
				</div>
			</CardContent>
		</Card>
	);
}
