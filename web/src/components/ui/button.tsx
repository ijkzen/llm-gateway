import { Slot } from "@radix-ui/react-slot";
import { type VariantProps, cva } from "class-variance-authority";
import * as React from "react";

import { cn } from "@/lib/utils";

/* 按钮规格对齐 Linear 官方 Button：圆角 8px；悬停变亮（brightness）而非变暗；
   按下 scale(.97)；尺寸 default 40px/15px、small 32px/13px、large 44px/16px；
   纯图标 icon 40px / iconSm 32px（与 small 同高，图标同为 16px）；
   secondary 用 backdrop-blur(4px) + 内高光与描边（Linear 原样，仅小面积元素使用）。 */
const buttonVariants = cva(
	"inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-md text-[15px] font-medium outline-none transition-colors duration-[160ms] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 enabled:active:scale-[0.97] [&_svg]:pointer-events-none [&_svg]:size-[18px] [&_svg]:shrink-0",
	{
		variants: {
			variant: {
				default:
					"bg-primary text-primary-foreground hover:brightness-[1.15] active:brightness-[0.98]",
				destructive:
					"bg-destructive text-destructive-foreground hover:brightness-[1.08] active:brightness-[0.98]",
				/* Linear secondary：暗色为半透明 + blur(4px) + 内高光/外描边；亮色为实底 + 细环 */
				secondary:
					"bg-background text-foreground shadow-[0_0_0_1px_rgba(0,0,0,0.08),0_1px_2px_rgba(0,0,0,0.06)] hover:bg-accent dark:bg-white/[0.055] dark:shadow-[inset_0_0_0_1px_rgba(255,255,255,0.032),inset_0_1px_0_rgba(255,255,255,0.04),0_0_0_1px_rgba(0,0,0,0.6),0_4px_4px_rgba(0,0,0,0.1)] dark:backdrop-blur-sm dark:hover:bg-white/[0.09]",
				/* Linear tertiary：描边 + 底色，静止态弱化文字，悬停提亮 */
				outline:
					"border border-border bg-background text-muted-foreground hover:border-input hover:bg-accent hover:text-foreground",
				ghost: "text-muted-foreground hover:bg-accent hover:text-foreground active:bg-accent",
				link: "text-primary underline-offset-4 hover:underline",
			},
			size: {
				default: "h-10 px-4 py-2",
				sm: "h-8 gap-2 rounded-md px-3 text-[13px] [&_svg]:size-4",
				lg: "h-11 rounded-md px-5 text-[16px]",
				icon: "size-10",
				iconSm: "size-8 [&_svg]:size-4",
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
