import { useTranslation } from "react-i18next";

export function SkipToMain() {
	const { t } = useTranslation();
	return (
		<a
			href="#content"
			className="fixed left-4 top-4 z-[999] -translate-y-52 whitespace-nowrap rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground opacity-95 shadow-sm transition hover:bg-primary/90 focus:translate-y-0 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
		>
			{t("common.skipToMain")}
		</a>
	);
}
