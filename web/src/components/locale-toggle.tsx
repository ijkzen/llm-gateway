import { Languages } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useChangeLocale } from "@/hooks/use-locale";
import type { Locale } from "@/i18n";

const LOCALE_OPTIONS: { value: Locale; labelKey: string }[] = [
	{ value: "zh-CN", labelKey: "language.zhCN" },
	{ value: "en", labelKey: "language.en" },
];

/** 顶栏/登录页共用的语言切换入口（Languages 图标 + 下拉）。 */
export default function LocaleToggle() {
	const { t } = useTranslation();
	const changeLocale = useChangeLocale();
	const [open, setOpen] = useState(false);

	return (
		<DropdownMenu open={open} onOpenChange={setOpen}>
			<DropdownMenuTrigger asChild>
				<Button
					variant="ghost"
					size="icon"
					className="size-8"
					aria-label={t("language.switchLanguage")}
				>
					<Languages className="size-4" />
				</Button>
			</DropdownMenuTrigger>
			<DropdownMenuContent align="end">
				{LOCALE_OPTIONS.map((option) => (
					<DropdownMenuItem
						key={option.value}
						onSelect={() => {
							void changeLocale(option.value);
						}}
					>
						{t(option.labelKey)}
					</DropdownMenuItem>
				))}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}
