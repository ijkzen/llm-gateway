import { useTheme } from "@/hooks/use-theme";
import { Toaster as Sonner, type ToasterProps } from "sonner";

export function Toaster({ ...props }: ToasterProps) {
	const theme = useTheme((state) => state.theme);

	return (
		<Sonner
			theme={theme as ToasterProps["theme"]}
			richColors
			duration={5000}
			className="toaster group"
			style={
				{
					"--normal-bg": "hsl(var(--popover))",
					"--normal-text": "hsl(var(--popover-foreground))",
					"--normal-border": "hsl(var(--border))",
				} as React.CSSProperties
			}
			toastOptions={{
				classNames: {
					toast:
						"!rounded-xl !border-white/70 !bg-white/90 !shadow-[0_16px_36px_rgba(15,23,42,0.14),inset_0_1px_0_rgba(255,255,255,0.9)] !backdrop-blur-xl dark:!border-white/12 dark:!bg-[#151823]/95",
				},
			}}
			{...props}
		/>
	);
}
