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
			{...props}
		/>
	);
}
