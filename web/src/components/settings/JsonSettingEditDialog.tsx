import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { type Setting, useUpdateSetting } from "@/hooks/use-settings";
import { useToastActions } from "@/hooks/use-toast";
import { Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

/** 结构化编辑模式：字符串数组 / 键值对对象；不符（嵌套等）退回原文编辑。 */
type JsonEditMode = "array" | "object" | "raw";

interface JsonRow {
	key: string;
	value: string;
}

function parseJsonMode(value: string): { mode: JsonEditMode; rows: JsonRow[] } {
	try {
		const parsed: unknown = JSON.parse(value);
		if (Array.isArray(parsed) && parsed.every((entry) => typeof entry === "string")) {
			return {
				mode: "array",
				rows: parsed.map((entry) => ({ key: "", value: entry })),
			};
		}
		if (
			parsed !== null &&
			typeof parsed === "object" &&
			Object.values(parsed).every((v) => typeof v === "string")
		) {
			return {
				mode: "object",
				rows: Object.entries(parsed).map(([key, value]) => ({
					key,
					value: value as string,
				})),
			};
		}
	} catch {
		// 非法 JSON 走原文编辑，保存时由后端校验兜底。
	}
	return { mode: "raw", rows: [] };
}

interface JsonSettingEditDialogProps {
	setting: Setting | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}

export function JsonSettingEditDialog({ setting, open, onOpenChange }: JsonSettingEditDialogProps) {
	const { toastSuccess, toastError } = useToastActions();
	const updateSetting = useUpdateSetting();
	const [mode, setMode] = useState<JsonEditMode>("raw");
	const [rows, setRows] = useState<JsonRow[]>([]);
	const [rawValue, setRawValue] = useState("");

	useEffect(() => {
		if (open && setting) {
			const parsed = parseJsonMode(setting.value);
			setMode(parsed.mode);
			setRows(parsed.rows);
			setRawValue(parsed.mode === "raw" ? setting.value : "");
		}
	}, [open, setting]);

	const updateRow = (index: number, patch: Partial<JsonRow>) => {
		setRows((prev) => prev.map((row, i) => (i === index ? { ...row, ...patch } : row)));
	};

	const addRow = () => setRows((prev) => [...prev, { key: "", value: "" }]);

	const removeRow = (index: number) => setRows((prev) => prev.filter((_, i) => i !== index));

	const handleSave = () => {
		if (!setting) return;
		let value: string;
		if (mode === "array") {
			value = JSON.stringify(rows.map((row) => row.value.trim()).filter((v) => v !== ""));
		} else if (mode === "object") {
			const obj: Record<string, string> = {};
			for (const row of rows) {
				const key = row.key.trim();
				if (key) obj[key] = row.value;
			}
			value = JSON.stringify(obj);
		} else {
			value = rawValue.trim();
		}
		updateSetting.mutate(
			{ key: setting.key, value },
			{
				onSuccess: () => {
					onOpenChange(false);
					toastSuccess("更新成功");
				},
				onError: (error) => {
					toastError("更新失败", error);
				},
			},
		);
	};

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent className="sm:max-w-[560px]">
				<DialogHeader className="space-y-3">
					<DialogTitle>编辑 JSON 设置</DialogTitle>
					<p className="font-mono text-xs text-muted-foreground">{setting?.key}</p>
				</DialogHeader>
				<form
					onSubmit={(e) => {
						e.preventDefault();
						handleSave();
					}}
					className="flex min-h-0 flex-col"
				>
					<div className="max-h-[50vh] min-h-0 space-y-2 overflow-y-auto py-2">
						{mode === "raw" ? (
							<p className="text-sm text-muted-foreground">
								该 JSON 包含嵌套或非字符串值，请直接编辑原文：
							</p>
						) : null}
						{mode === "raw" ? (
							<textarea
								value={rawValue}
								onChange={(e) => setRawValue(e.target.value)}
								rows={6}
								className="flex w-full rounded-lg border border-input bg-white/70 px-3 py-2 font-mono text-xs shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:bg-white/5"
							/>
						) : (
							rows.map((row, index) => (
								// biome-ignore lint/suspicious/noArrayIndexKey: 行随增删整体重排，无稳定 id
								<div key={index} className="flex items-center gap-2">
									{mode === "object" ? (
										<Input
											placeholder="键"
											value={row.key}
											onChange={(e) => updateRow(index, { key: e.target.value })}
											className="flex-1 font-mono text-xs"
										/>
									) : null}
									<Input
										placeholder="值"
										value={row.value}
										onChange={(e) => updateRow(index, { value: e.target.value })}
										className="flex-1 font-mono text-xs"
									/>
									<Button
										type="button"
										variant="ghost"
										size="icon"
										aria-label="删除"
										onClick={() => removeRow(index)}
									>
										<Trash2 className="size-4 text-muted-foreground" />
									</Button>
								</div>
							))
						)}
						{mode !== "raw" && rows.length === 0 ? (
							<p className="text-sm text-muted-foreground">暂无条目，点击「新增」添加。</p>
						) : null}
					</div>
					{mode !== "raw" ? (
						<Button
							type="button"
							variant="outline"
							size="sm"
							onClick={addRow}
							className="mt-2 w-fit"
						>
							<Plus className="size-4" />
							新增
						</Button>
					) : null}
					<DialogFooter className="gap-2">
						<Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
							取消
						</Button>
						<Button type="submit" disabled={updateSetting.isPending}>
							保存
						</Button>
					</DialogFooter>
				</form>
			</DialogContent>
		</Dialog>
	);
}
