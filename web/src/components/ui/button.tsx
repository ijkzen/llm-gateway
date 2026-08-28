import { Slot } from "@radix-ui/react-slot";
import { type VariantProps, cva } from "class-variance-authority";
import * as React from "react";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
	"inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-medium ring-offset-background transition-[color,background-color,border-color,box-shadow,transform] duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 active:enabled:translate-y-px",
	{
		variants: {
			variant: {
				default:
					"bg-primary text-primary-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.15),0_2px_6px_rgba(15,23,42,0.16)] hover:bg-primary/90",
				destructive:
					"bg-destructive text-destructive-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.2)] hover:bg-destructive/90",
				outline:
					"border border-white/70 bg-white/60 text-foreground shadow-[0_1px_2px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.8)] backdrop-blur-sm hover:bg-white/90 hover:text-accent-foreground dark:border-white/12 dark:bg-white/5 dark:hover:bg-white/10",
				secondary:
					"bg-secondary text-secondary-foreground shadow-[inset_0_1px_0_rgba(255,255,255,0.6)] hover:bg-secondary/80 dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.06)]",
				ghost: "hover:bg-accent hover:text-accent-foreground",
				link: "text-primary underline-offset-4 hover:underline",
			},
			size: {
				default: "h-10 px-4 py-2",
				sm: "h-9 rounded-lg px-3",
				lg: "h-11 rounded-lg px-8",
				icon: "h-10 w-10",
			},
		},
		defaultVariants: {
			variant: "default",
			size: "default",
		},
	},
);

export interface ButtonProps
	extends React.ButtonHTMLAttributes<HTMLButtonElement>,
		VariantProps<typeof buttonVariants> {
	asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
	({ className, variant, size, asChild = false, ...props }, ref) => {
		const Comp = asChild ? Slot : "button";
		return (
			<Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />
		);
	},
);
Button.displayName = "Button";

export { Button, buttonVariants };
