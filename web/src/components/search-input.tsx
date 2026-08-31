import { Input } from "@/components/ui/input";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";

interface SearchInputProps {
	value: string;
	onChange: (value: string) => void;
	placeholder?: string;
	"aria-label"?: string;
}

export function SearchInput({
	value,
	onChange,
	placeholder,
	"aria-label": ariaLabel,
}: SearchInputProps) {
	const { t } = useTranslation();
	const resolvedPlaceholder = placeholder ?? t("common.searchPlaceholder");
	return (
		<div className="relative">
			<Search className="absolute left-2.5 top-2.5 size-4 text-muted-foreground" />
			<Input
				type="search"
				placeholder={resolvedPlaceholder}
				aria-label={ariaLabel ?? resolvedPlaceholder}
				value={value}
				onChange={(e) => onChange(e.target.value)}
				className="pl-9"
			/>
		</div>
	);
}
