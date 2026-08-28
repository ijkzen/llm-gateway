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

const THEME_OPTIONS: { value: Theme; label: string; icon: typeof Sun }[] = [
	{ value: "light", label: "亮色", icon: Sun },
	{ value: "dark", label: "暗色", icon: Moon },
	{ value: "system", label: "跟随系统", icon: Monitor },
];

export function ThemeToggle() {
	const buttonRef = useRef<HTMLButtonElement>(null);
	const theme = useTheme((state) => state.theme);
	const setTheme = useTheme((state) => state.setTheme);

	return (
		<DropdownMenu modal={false}>
			<DropdownMenuTrigger asChild>
				<Button ref={buttonRef} variant="ghost" size="icon" aria-label="切换主题">
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
				{THEME_OPTIONS.map(({ value, label, icon: Icon }) => (
					<DropdownMenuItem key={value} onClick={() => setTheme(value, buttonRef)}>
						<Icon className="size-4" />
						{label}
						<Check className={cn("ml-auto size-3.5", theme !== value && "invisible")} />
					</DropdownMenuItem>
				))}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}
