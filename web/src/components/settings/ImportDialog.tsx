import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { useToastActions } from "@/hooks/use-toast";
import { importBackup } from "@/lib/backup";
import { useQueryClient } from "@tanstack/react-query";
import { FileUp, FolderOpen } from "lucide-react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

interface ImportDialogProps {
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

interface FileState {
	name: string;
	text: string;
}

/** 拖拽区域是否高亮（dragover 中）。 */
function isDragOver(event: React.DragEvent): boolean {
	return event.dataTransfer?.types?.includes("Files") ?? false;
}

/**
 * 恢复备份导入弹窗：上部拖拽区 + 下部手动选择文件 + 底部「确认导入」。
 * 确认前弹破坏性确认（整体替换），成功 Toast，失败/格式错误弹具体错误弹窗。
 * 导入成功后失效全部配置查询。
 */
export function ImportDialog({ open, onOpenChange }: ImportDialogProps) {
	const { t } = useTranslation();
	const { toastSuccess } = useToastActions();
	const queryClient = useQueryClient();
	const inputRef = useRef<HTMLInputElement>(null);
	const [file, setFile] = useState<FileState | null>(null);
	const [dragging, setDragging] = useState(false);
	const [confirmOpen, setConfirmOpen] = useState(false);
	const [importing, setImporting] = useState(false);
	/** 导入失败时的具体错误消息（null = 无错误弹窗）。 */
	const [errorMessage, setErrorMessage] = useState<string | null>(null);

	const readFile = (f: File) => {
		if (!f) return;
		const reader = new FileReader();
		reader.onload = () => {
			setFile({ name: f.name, text: String(reader.result ?? "") });
		};
		reader.readAsText(f);
	};

	const handleDrop = (e: React.DragEvent) => {
		e.preventDefault();
		setDragging(false);
		const f = e.dataTransfer.files?.[0];
		if (f) readFile(f);
	};

	const handleChoose = () => inputRef.current?.click();

	/** 18-14：清空已选文件（含隐藏 input 的 value，否则重选同一文件不触发 change）。 */
	const clearFile = () => {
		setFile(null);
		if (inputRef.current) inputRef.current.value = "";
	};

	const doImport = async () => {
		if (!file) return;
		setImporting(true);
		try {
			await importBackup(file.text);
			toastSuccess(t("backup.importSuccess"));
			clearFile();
			onOpenChange(false);
			queryClient.invalidateQueries();
		} catch (error) {
			// 后端 400 的具体中文错误直接展示；网络等其它错误给通用文案。
			const msg = error instanceof Error ? error.message : "";
			setErrorMessage(msg || t("backup.errorDialogBody"));
		} finally {
			setImporting(false);
		}
	};

	const handleConfirm = () => {
		setConfirmOpen(false);
		void doImport();
	};

	return (
		<>
			{/* 18-14：弹窗关闭（含取消）后清空已选文件，重开不再残留旧文件可直接导入。 */}
			<Dialog
				open={open}
				onOpenChange={(next) => {
					if (!next) clearFile();
					onOpenChange(next);
				}}
			>
				<DialogContent className="sm:max-w-[520px]">
					<DialogHeader>
						<DialogTitle>{t("backup.importDialogTitle")}</DialogTitle>
						<DialogDescription>{t("backup.importHint")}</DialogDescription>
					</DialogHeader>

					<div className="grid gap-4 py-2">
						{/* 上半部分：拖拽区（点击同样打开文件选择） */}
						<button
							type="button"
							aria-label={t("backup.dropzoneLabel")}
							onClick={handleChoose}
							onDragOver={(e) => {
								e.preventDefault();
								if (isDragOver(e)) setDragging(true);
							}}
							onDragLeave={() => setDragging(false)}
							onDrop={handleDrop}
							className={`flex h-32 cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed transition-colors ${
								dragging ? "border-primary bg-muted/60" : "border-muted-foreground/40"
							}`}
						>
							<FileUp className="size-8 text-muted-foreground" />
							<span className="text-sm text-muted-foreground">{t("backup.dropzoneLabel")}</span>
						</button>

						{/* 下半部分：手动选择文件 + 已选文件 */}
						<div className="flex flex-col items-center gap-2">
							<Button type="button" variant="outline" onClick={handleChoose}>
								<FolderOpen className="size-4" />
								{t("backup.chooseFile")}
							</Button>
							<input
								ref={inputRef}
								type="file"
								accept=".json,application/json"
								className="hidden"
								onChange={(e) => {
									const f = e.target.files?.[0];
									if (f) readFile(f);
								}}
							/>
							<span className="text-xs text-muted-foreground">
								{file ? `${t("backup.selectedFile")} ${file.name}` : t("backup.noFile")}
							</span>
						</div>
					</div>

					<DialogFooter>
						<Button
							type="button"
							disabled={!file || importing}
							onClick={() => setConfirmOpen(true)}
						>
							{importing ? t("common.loading") : t("backup.confirmImport")}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>

			{/* 破坏性确认 */}
			<AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>{t("backup.confirmImportTitle")}</AlertDialogTitle>
						<AlertDialogDescription>{t("backup.confirmImportBody")}</AlertDialogDescription>
					</AlertDialogHeader>
					<AlertDialogFooter>
						<AlertDialogCancel>{t("backup.confirmCancel")}</AlertDialogCancel>
						<AlertDialogAction onClick={handleConfirm}>{t("backup.confirmOk")}</AlertDialogAction>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>

			{/* 失败/格式错误详情弹窗 */}
			<AlertDialog
				open={errorMessage !== null}
				onOpenChange={(next) => !next && setErrorMessage(null)}
			>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>{t("backup.errorDialogTitle")}</AlertDialogTitle>
						<AlertDialogDescription>{errorMessage}</AlertDialogDescription>
					</AlertDialogHeader>
					<AlertDialogFooter>
						<AlertDialogAction onClick={() => setErrorMessage(null)}>
							{t("common.close")}
						</AlertDialogAction>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>
		</>
	);
}
