import {
	AlertDialog,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useTranslation } from "react-i18next";

interface ConfirmDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	title?: React.ReactNode;
	desc?: React.ReactNode;
	cancelBtnText?: string;
	confirmText?: React.ReactNode;
	destructive?: boolean;
	disabled?: boolean;
	isLoading?: boolean;
	className?: string;
	children?: React.ReactNode;
	handleConfirm: () => void;
}

export function ConfirmDialog({
	open,
	onOpenChange,
	title,
	desc,
	cancelBtnText,
	confirmText,
	destructive = false,
	disabled = false,
	isLoading = false,
	className,
	children,
	handleConfirm,
}: ConfirmDialogProps) {
	const { t } = useTranslation();
	const resolvedTitle = title ?? t("cronJobs.deleteTitle");
	const resolvedDesc = desc ?? t("common.cannotUndo");
	const resolvedCancelBtnText = cancelBtnText ?? t("common.cancel");
	const resolvedConfirmText = confirmText ?? t("common.confirm");
	return (
		<AlertDialog open={open} onOpenChange={onOpenChange}>
			<AlertDialogContent className={cn(className)}>
				<AlertDialogHeader className="space-y-3 text-left">
					<AlertDialogTitle>{resolvedTitle}</AlertDialogTitle>
					<AlertDialogDescription asChild={typeof resolvedDesc !== "string"}>
						{typeof resolvedDesc === "string" ? resolvedDesc : <div>{resolvedDesc}</div>}
					</AlertDialogDescription>
				</AlertDialogHeader>
				{children}
				<AlertDialogFooter className="gap-2">
					<AlertDialogCancel disabled={isLoading}>{resolvedCancelBtnText}</AlertDialogCancel>
					<Button
						type="button"
						variant={destructive ? "destructive" : "default"}
						disabled={disabled || isLoading}
						onClick={handleConfirm}
					>
						{resolvedConfirmText}
					</Button>
				</AlertDialogFooter>
			</AlertDialogContent>
		</AlertDialog>
	);
}
