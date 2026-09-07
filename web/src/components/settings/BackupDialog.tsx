import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { useToastActions } from "@/hooks/use-toast";
import { fetchBackupExport } from "@/lib/backup";
import { ArchiveRestore, Download } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ImportDialog } from "./ImportDialog";

interface BackupDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

/** 触发浏览器下载备份 JSON 文件（文件名含日期，便于区分多次备份）。 */
function downloadBackupFile(json: string) {
	const blob = new Blob([json], { type: "application/json" });
	const url = URL.createObjectURL(blob);
	const a = document.createElement("a");
	a.href = url;
	a.download = `llm-gateway-backup-${new Date().toISOString().slice(0, 10)}.json`;
	document.body.appendChild(a);
	a.click();
	document.body.removeChild(a);
	URL.revokeObjectURL(url);
}

/**
 * 备份入口弹窗：两个动作「导出备份」（下载 JSON）与「恢复备份」
 * （切换到导入弹窗）。
 */
export function BackupDialog({ open, onOpenChange }: BackupDialogProps) {
	const { t } = useTranslation();
	const { toastError } = useToastActions();
	const [exporting, setExporting] = useState(false);
	const [showImport, setShowImport] = useState(false);

	const handleExport = async () => {
		setExporting(true);
		try {
			const data = await fetchBackupExport();
			downloadBackupFile(JSON.stringify(data, null, 2));
		} catch (error) {
			toastError(
				t("backup.exportFailed"),
				error instanceof Error ? error : new Error(String(error)),
			);
		} finally {
			setExporting(false);
		}
	};

	// 点「恢复备份」→ 关闭本弹窗并打开导入弹窗（由父级统一切换，见 settings 页）。
	const handleOpenImport = () => {
		onOpenChange(false);
		setShowImport(true);
	};

	return (
		<>
			<Dialog open={open} onOpenChange={onOpenChange}>
				<DialogContent className="sm:max-w-[460px]">
					<DialogHeader>
						<DialogTitle>{t("backup.title")}</DialogTitle>
						<DialogDescription>{t("backup.exportHint")}</DialogDescription>
					</DialogHeader>
					<div className="grid gap-3 py-2">
						<Button variant="outline" onClick={handleExport} disabled={exporting}>
							<Download className="size-4" />
							{exporting ? t("common.loading") : t("backup.export")}
						</Button>
						<Button variant="outline" onClick={handleOpenImport}>
							<ArchiveRestore className="size-4" />
							{t("backup.import")}
						</Button>
					</div>
				</DialogContent>
			</Dialog>
			{/* 导入弹窗：备份弹窗关闭后由这里打开；关闭时同步复位本弹窗状态。 */}
			<ImportDialog
				open={showImport}
				onOpenChange={(next) => {
					setShowImport(next);
					if (!next) onOpenChange(false);
				}}
			/>
		</>
	);
}
