import { Button } from "@/components/ui/button";
import { AlertCircle, RefreshCw } from "lucide-react";

interface ErrorStateProps {
	title?: string;
	description?: string;
	onRetry?: () => void;
}

export function ErrorState({ title = "加载失败", description, onRetry }: ErrorStateProps) {
	return (
		<div className="rounded-xl border border-destructive/50 bg-destructive/10 p-6 text-center">
			<AlertCircle className="mx-auto size-10 text-destructive" />
			<h3 className="mt-4 text-lg font-semibold">{title}</h3>
			{description && <p className="mt-2 text-muted-foreground">{description}</p>}
			{onRetry && (
				<Button className="mt-4" variant="outline" size="sm" onClick={onRetry}>
					<RefreshCw className="mr-2 size-4" />
					重试
				</Button>
			)}
		</div>
	);
}
