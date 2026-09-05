import { MidEllipsis } from "@/components/mid-ellipsis";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import { ChevronDown, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

export interface MultiSelectOption {
	value: string;
	label: string;
	/** 可选分组标题（如按供应商分组）；同一组的连续选项共享一个标题行。 */
	group?: string;
}

interface MultiSelectProps {
	options: MultiSelectOption[];
	/** 选中值集合；空数组 = 全部。 */
	selected: string[];
	/** 勾满全部选项时回调空数组（归一化为「全部」）。 */
	onChange: (selected: string[]) => void;
	"aria-label"?: string;
	className?: string;
}

export function MultiSelect({ options, selected, onChange, className, ...rest }: MultiSelectProps) {
	const { t } = useTranslation();
	const [open, setOpen] = useState(false);
	const [keyword, setKeyword] = useState("");

	const allValues = useMemo(() => options.map((o) => o.value), [options]);
	// 空选择 = 全部（隐式全选）。selected 为空时视为勾满全部，UI 上只有「全选」为勾选态，
	// 子条目全部显示未勾选；直接点某子条目即从「全部」退化为仅选该项。
	const isAll = selected.length === 0;
	const selectedSet = useMemo(() => new Set(selected), [selected]);
	const allChecked = isAll || (options.length > 0 && selectedSet.size === options.length);
	const someChecked = !isAll && !allChecked && selectedSet.size > 0;

	const commit = (next: Set<string>) => {
		const kept = allValues.filter((v) => next.has(v));
		onChange(kept.length === allValues.length ? [] : kept);
	};

	const toggle = (value: string) => {
		if (isAll) {
			// 全选态点击子条目：退化为仅选该项（不等同于「除它以外全部」）。
			onChange([value]);
			return;
		}
		const next = new Set(selectedSet);
		if (next.has(value)) {
			next.delete(value);
		} else {
			next.add(value);
		}
		commit(next);
	};

	const visible = useMemo(() => {
		const kw = keyword.trim().toLowerCase();
		if (!kw) return options;
		return options.filter((o) => o.label.toLowerCase().includes(kw));
	}, [keyword, options]);

	type Row = { kind: "header"; label: string } | { kind: "option"; option: MultiSelectOption };
	const rows = useMemo<Row[]>(() => {
		const result: Row[] = [];
		let lastGroup: string | undefined;
		for (const option of visible) {
			if (option.group !== undefined && option.group !== lastGroup) {
				result.push({ kind: "header", label: option.group });
				lastGroup = option.group;
			}
			result.push({ kind: "option", option });
		}
		return result;
	}, [visible]);

	const triggerLabel =
		selected.length === 0
			? t("common.all")
			: selected.length === 1
				? (options.find((o) => o.value === selected[0])?.label ?? selected[0])
				: t("multiSelect.selectedCount", { count: selected.length });

	return (
		<Popover
			open={open}
			onOpenChange={(next) => {
				setOpen(next);
				if (!next) setKeyword("");
			}}
		>
			<PopoverTrigger asChild>
				<Button
					type="button"
					variant="outline"
					aria-haspopup="dialog"
					aria-expanded={open}
					className={cn("justify-between font-normal", className)}
					{...rest}
				>
					<MidEllipsis text={triggerLabel ?? ""} />
					<ChevronDown className="ml-1 size-4 shrink-0 opacity-50" />
				</Button>
			</PopoverTrigger>
			<PopoverContent align="start" className="w-[200px] space-y-1 p-2">
				<div className="relative">
					<Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
					<input
						value={keyword}
						onChange={(e) => setKeyword(e.target.value)}
						placeholder={t("multiSelect.searchPlaceholder")}
						className="h-8 w-full rounded-md border border-input bg-transparent pl-7 pr-2 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring"
					/>
				</div>
				<label
					htmlFor="multi-select-select-all"
					className="flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-sm hover:bg-accent"
				>
					<Checkbox
						id="multi-select-select-all"
						checked={allChecked ? true : someChecked ? "indeterminate" : false}
						onCheckedChange={() => onChange([])}
						aria-label={t("multiSelect.selectAll")}
					/>
					{t("multiSelect.selectAll")}
				</label>
				<div className="max-h-64 space-y-0.5 overflow-y-auto">
					{rows.map((row) =>
						row.kind === "header" ? (
							<p
								key={`header-${row.label}`}
								className="px-1.5 pt-1 pb-0.5 text-xs text-muted-foreground"
							>
								{row.label}
							</p>
						) : (
							<label
								key={row.option.value}
								htmlFor={`multi-select-${row.option.value}`}
								className="flex cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-sm hover:bg-accent"
							>
								<Checkbox
									id={`multi-select-${row.option.value}`}
									aria-label={row.option.label}
									checked={isAll ? false : selectedSet.has(row.option.value)}
									onCheckedChange={() => toggle(row.option.value)}
								/>
								<MidEllipsis text={row.option.label} />
							</label>
						),
					)}
					{visible.length === 0 && (
						<p className="px-1.5 py-2 text-center text-sm text-muted-foreground">
							{t("multiSelect.noMatch")}
						</p>
					)}
				</div>
			</PopoverContent>
		</Popover>
	);
}
