import { Input } from "@/components/ui/input";
import { Search } from "lucide-react";

interface SearchInputProps {
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
	"aria-label"?: string;
}

export function SearchInput({
	value,
	onChange,
	placeholder = "搜索...",
	"aria-label": ariaLabel,
}: SearchInputProps) {
	return (
		<div className="relative">
			<Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
			<Input
				type="search"
				placeholder={placeholder}
				aria-label={ariaLabel ?? placeholder}
				value={value}
				onChange={(e) => onChange(e.target.value)}
				className="pl-9"
			/>
		</div>
	);
}
