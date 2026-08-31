import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { type Theme, useTheme } from "@/hooks/use-theme";
import { cn } from "@/lib/utils";
import { Check, Monitor, Moon, Sun } from "lucide-react";
import { useRef } from "react";
import { useTranslation } from "react-i18next";

const THEME_OPTIONS: { value: Theme; labelKey: string; icon: typeof Sun }[] = [
	{ value: "light", labelKey: "theme.light", icon: Sun },
	{ value: "dark", labelKey: "theme.dark", icon: Moon },
	{ value: "system", labelKey: "theme.system", icon: Monitor },
];

export function ThemeToggle() {
	const { t } = useTranslation();
	const buttonRef = useRef<HTMLButtonElement>(null);
	const theme = useTheme((state) => state.theme);
	const setTheme = useTheme((state) => state.setTheme);

	return (
		<DropdownMenu modal={false}>
			<DropdownMenuTrigger asChild>
				<Button ref={buttonRef} variant="ghost" size="icon" aria-label={t("theme.switchTheme")}>
					{theme === "dark" ? (
						<Moon className="size-4" />
					) : theme === "system" ? (
						<Monitor className="size-4" />
					) : (
						<Sun className="size-4" />
					)}
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end">
				{THEME_OPTIONS.map(({ value, labelKey, icon: Icon }) => (
					<DropdownMenuItem key={value} onClick={() => setTheme(value, buttonRef)}>
						<Icon className="size-4" />
						{t(labelKey)}
						<Check className={cn("ml-auto size-3.5", theme !== value && "invisible")} />
					</DropdownMenuItem>
				))}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}
