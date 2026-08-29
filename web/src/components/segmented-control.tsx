import { cn } from "@/lib/utils";

interface SegmentedControlProps<T extends string> {
	options: readonly { value: T; label: string }[];
	value: T;
	onChange: (value: T) => void;
}

/** 三态/多态分段开关，用于图表视图切换。 */
export function SegmentedControl<T extends string>({
	options,
	value,
	onChange,
}: SegmentedControlProps<T>) {
	return (
		<div className="flex items-center gap-1 rounded-full bg-foreground/5 p-1 dark:bg-white/5">
			{options.map((option) => (
				<button
					key={option.value}
					type="button"
					aria-pressed={value === option.value}
					onClick={() => onChange(option.value)}
					className={cn(
						"rounded-full px-3 py-1 text-xs font-medium text-muted-foreground transition-colors",
						value === option.value &&
							"bg-background text-foreground shadow-[0_1px_3px_rgba(15,23,42,0.12)]",
					)}
				>
					{option.label}
				</button>
			))}
		</div>
	);
}
