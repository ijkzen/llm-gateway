import { Button } from "@/components/ui/button";
import { AlertCircle, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ErrorStateProps {
	title?: string;
	description?: string;
	onRetry?: () => void;
}

export function ErrorState({ title, description, onRetry }: ErrorStateProps) {
	const { t } = useTranslation();
	return (
		<div className="rounded-2xl border border-destructive/30 bg-destructive/[0.06] p-6 text-center backdrop-blur-xl dark:bg-destructive/10">
			<AlertCircle className="mx-auto size-10 text-destructive" />
			<h3 className="mt-4 text-lg font-semibold">{title ?? t("common.loadFailed")}</h3>
			{description && <p className="mt-2 text-muted-foreground">{description}</p>}
			{onRetry && (
				<Button className="mt-4" variant="outline" size="sm" onClick={onRetry}>
					<RefreshCw className="mr-2 size-4" />
					{t("common.retry")}
				</Button>
			)}
		</div>
	);
}
