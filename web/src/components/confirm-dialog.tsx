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
	title = "确认删除",
	desc = "此操作无法撤销。",
	cancelBtnText = "取消",
	confirmText = "确认",
	destructive = false,
	disabled = false,
	isLoading = false,
	className,
	children,
	handleConfirm,
}: ConfirmDialogProps) {
	return (
		<AlertDialog open={open} onOpenChange={onOpenChange}>
			<AlertDialogContent className={cn(className)}>
				<AlertDialogHeader className="space-y-3 text-left">
					<AlertDialogTitle>{title}</AlertDialogTitle>
					<AlertDialogDescription asChild={typeof desc !== "string"}>
						{typeof desc === "string" ? desc : <div>{desc}</div>}
					</AlertDialogDescription>
				</AlertDialogHeader>
				{children}
				<AlertDialogFooter className="gap-2">
					<AlertDialogCancel disabled={isLoading}>{cancelBtnText}</AlertDialogCancel>
					<Button
						type="button"
						variant={destructive ? "destructive" : "default"}
						disabled={disabled || isLoading}
						onClick={handleConfirm}
					>
						{confirmText}
					</Button>
				</AlertDialogFooter>
			</AlertDialogContent>
		</AlertDialog>
	);
}
