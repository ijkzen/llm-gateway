# Neumorphism Design System Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply the approved Neumorphism (Soft UI) design system to the existing React admin frontend (`web/`) by adding tokens, creating a parallel `Neu*` component layer, and restyling the layout plus the Cron Jobs and Settings pages.

**Architecture:** Design tokens live in CSS variables inside `web/src/index.css` and are exposed through `web/tailwind.config.ts`. A new `web/src/components/neu/` folder holds non-destructive Neumorphic primitives and layout components. Existing shadcn/ui files remain untouched. `web/src/App.tsx` swaps in `NeuLayout`, and the two page components are rewritten to use card lists instead of tables while keeping all TanStack Query logic.

**Tech Stack:** React 19, TypeScript 5.6, Vite 6, Tailwind CSS 3.4, class-variance-authority, clsx, tailwind-merge, Framer Motion (already installed), Google Fonts (Plus Jakarta Sans + DM Sans).

---

## File Structure

| File | Responsibility |
|------|----------------|
| `web/index.html` | Load Plus Jakarta Sans + DM Sans with `display=swap`. |
| `web/src/index.css` | Add `:root` and `.dark` CSS variables for Neu colors, shadows, and reusable shadow utility classes. |
| `web/tailwind.config.ts` | Extend Tailwind theme with Neu colors, fonts, radius aliases, and a `float` keyframe animation. |
| `web/src/components/neu/card.tsx` | Extruded rounded card container. |
| `web/src/components/neu/icon-well.tsx` | Inset-deep container for icons. |
| `web/src/components/neu/button.tsx` | Extruded button with primary/secondary variants and active inset state. |
| `web/src/components/neu/icon-button.tsx` | Square extruded button for icon actions. |
| `web/src/components/neu/input.tsx` | Inset input with inset-deep focus and accent ring. |
| `web/src/components/neu/badge.tsx` | Inset pill badge. |
| `web/src/components/neu/switch.tsx` | Pill switch with sliding thumb. |
| `web/src/components/neu/dialog.tsx` | Extruded dialog overlay + content. |
| `web/src/components/neu/layout.tsx` | `NeuLayout`, `NeuSidebar`, `NeuHeader`, `NeuMobileNav` sheet. |
| `web/src/App.tsx` | Use `NeuLayout` instead of the existing `AppLayout`. |
| `web/src/pages/cron-jobs.tsx` | Card-list Cron Jobs page using Neu components. |
| `web/src/pages/settings.tsx` | Card-list Settings page using Neu components. |

---

### Task 1: Load custom fonts

**Files:**
- Modify: `web/index.html`

Add the Google Fonts `<link>` inside `<head>` before the existing title/meta tags.

- [ ] **Step 1: Add Google Fonts import**

```html
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link
  href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@500;600;700;800&family=DM+Sans:opsz,wght@9..40,400;9..40,500;9..40,700&display=swap"
  rel="stylesheet"
/>
```

- [ ] **Step 2: Verify the link is inside `<head>`**

Run: `grep -n "fonts.googleapis" web/index.html`
Expected: lines found inside `<head>`.

- [ ] **Step 3: Commit**

```bash
git add web/index.html
git commit -m "feat(neu): load Plus Jakarta Sans and DM Sans fonts"
```

---

### Task 2: Add CSS design tokens and shadow utilities

**Files:**
- Modify: `web/src/index.css`

Add Neu tokens to the existing `:root` block and `.dark` block, then add a new `@layer utilities` block for reusable shadow classes.

- [ ] **Step 1: Add Neu variables to `:root`**

Inside the existing `:root { ... }` block, append these variables after the existing entries:

```css
  --neu-bg: #E0E5EC;
  --neu-fg: #3D4852;
  --neu-muted: #6B7280;
  --neu-accent: #6C63FF;
  --neu-accent-light: #8B84FF;
  --neu-success: #38B2AC;
  --neu-placeholder: #A0AEC0;
  --neu-shadow-light: rgba(255, 255, 255, 0.55);
  --neu-shadow-dark: rgba(163, 177, 198, 0.65);
```

- [ ] **Step 2: Add Neu variables to `.dark`**

Inside the existing `.dark { ... }` block, append these variables after the existing entries:

```css
  --neu-bg: #232931;
  --neu-fg: #E0E5EC;
  --neu-muted: #9CA3AF;
  --neu-accent: #8B84FF;
  --neu-accent-light: #A5A0FF;
  --neu-success: #4FD1C5;
  --neu-placeholder: #6B7280;
  --neu-shadow-light: rgba(255, 255, 255, 0.08);
  --neu-shadow-dark: rgba(0, 0, 0, 0.55);
```

- [ ] **Step 3: Add shadow utilities**

Append a new `@layer utilities` block at the end of `web/src/index.css`:

```css
@layer utilities {
  .neu-shadow {
    box-shadow: 9px 9px 16px var(--neu-shadow-dark), -9px -9px 16px var(--neu-shadow-light);
  }
  .neu-shadow-hover {
    box-shadow: 12px 12px 20px var(--neu-shadow-dark), -12px -12px 20px var(--neu-shadow-light);
  }
  .neu-shadow-sm {
    box-shadow: 5px 5px 10px var(--neu-shadow-dark), -5px -5px 10px var(--neu-shadow-light);
  }
  .neu-shadow-inset {
    box-shadow: inset 6px 6px 10px var(--neu-shadow-dark), inset -6px -6px 10px var(--neu-shadow-light);
  }
  .neu-shadow-inset-deep {
    box-shadow: inset 10px 10px 20px var(--neu-shadow-dark), inset -10px -10px 20px var(--neu-shadow-light);
  }
  .neu-shadow-inset-sm {
    box-shadow: inset 3px 3px 6px var(--neu-shadow-dark), inset -3px -3px 6px var(--neu-shadow-light);
  }
  .neu-ring {
    @apply ring-2 ring-[var(--neu-accent)] ring-offset-2 ring-offset-[var(--neu-bg)];
  }
}
```

- [ ] **Step 4: Update body background to use Neu background**

Change the existing `body` rule from:

```css
body {
  @apply bg-background text-foreground;
}
```

to:

```css
body {
  @apply bg-[var(--neu-bg)] text-[var(--neu-fg)];
  font-family: "DM Sans", sans-serif;
}
```

- [ ] **Step 5: Verify CSS compiles**

Run: `cd web && pnpm build`
Expected: build succeeds with no CSS errors.

- [ ] **Step 6: Commit**

```bash
git add web/src/index.css
git commit -m "feat(neu): add design tokens and shadow utilities"
```

---

### Task 3: Extend Tailwind config

**Files:**
- Modify: `web/tailwind.config.ts`

Add Neu colors, font families, a radius alias, and a `float` animation to the theme.

- [ ] **Step 1: Update Tailwind config**

Replace the contents of `web/tailwind.config.ts` with:

```typescript
import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        border: "hsl(var(--border))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        sidebar: {
          DEFAULT: "hsl(var(--sidebar-background))",
          foreground: "hsl(var(--sidebar-foreground))",
          primary: "hsl(var(--sidebar-primary))",
          "primary-foreground": "hsl(var(--sidebar-primary-foreground))",
          accent: "hsl(var(--sidebar-accent))",
          "accent-foreground": "hsl(var(--sidebar-accent-foreground))",
          border: "hsl(var(--sidebar-border))",
          ring: "hsl(var(--sidebar-ring))",
        },
        neu: {
          bg: "var(--neu-bg)",
          fg: "var(--neu-fg)",
          muted: "var(--neu-muted)",
          accent: "var(--neu-accent)",
          "accent-light": "var(--neu-accent-light)",
          success: "var(--neu-success)",
          placeholder: "var(--neu-placeholder)",
        },
      },
      fontFamily: {
        display: ["\"Plus Jakarta Sans\"", "sans-serif"],
        body: ["\"DM Sans\"", "sans-serif"],
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
        "3xl": "32px",
      },
      keyframes: {
        float: {
          "0%, 100%": { transform: "translateY(0)" },
          "50%": { transform: "translateY(-6px)" },
        },
      },
      animation: {
        float: "float 3s ease-in-out infinite",
      },
    },
  },
  plugins: [],
};

export default config;
```

- [ ] **Step 2: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/tailwind.config.ts
git commit -m "feat(neu): extend tailwind with neu colors, fonts, float animation"
```

---

### Task 4: Create NeuCard and NeuIconWell

**Files:**
- Create: `web/src/components/neu/card.tsx`
- Create: `web/src/components/neu/icon-well.tsx`

- [ ] **Step 1: Create NeuCard**

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface NeuCardProps extends React.HTMLAttributes<HTMLDivElement> {
  hover?: boolean;
}

export const NeuCard = React.forwardRef<HTMLDivElement, NeuCardProps>(
  ({ className, hover = false, children, ...props }, ref) => {
    return (
      <div
        ref={ref}
        className={cn(
          "rounded-3xl bg-neu-bg p-6 neu-shadow transition-all duration-300 ease-out",
          hover && "hover:-translate-y-0.5 hover:neu-shadow-hover",
          className,
        )}
        {...props}
      >
        {children}
      </div>
    );
  },
);
NeuCard.displayName = "NeuCard";
```

- [ ] **Step 2: Create NeuIconWell**

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface NeuIconWellProps extends React.HTMLAttributes<HTMLDivElement> {
  size?: "sm" | "md" | "lg";
}

const sizeClasses = {
  sm: "h-10 w-10 rounded-xl",
  md: "h-12 w-12 rounded-xl",
  lg: "h-14 w-14 rounded-2xl",
};

export const NeuIconWell = React.forwardRef<HTMLDivElement, NeuIconWellProps>(
  ({ className, size = "md", children, ...props }, ref) => {
    return (
      <div
        ref={ref}
        className={cn(
          "flex items-center justify-center text-neu-accent neu-shadow-inset-deep",
          sizeClasses[size],
          className,
        )}
        {...props}
      >
        {children}
      </div>
    );
  },
);
NeuIconWell.displayName = "NeuIconWell";
```

- [ ] **Step 3: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/neu/card.tsx web/src/components/neu/icon-well.tsx
git commit -m "feat(neu): add card and icon-well components"
```

---

### Task 5: Create NeuButton and NeuIconButton

**Files:**
- Create: `web/src/components/neu/button.tsx`
- Create: `web/src/components/neu/icon-button.tsx`

- [ ] **Step 1: Create NeuButton**

```tsx
import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const neuButtonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-2xl font-body font-medium text-sm transition-all duration-300 ease-out focus-visible:outline-none focus-visible:neu-ring disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0 active:translate-y-[0.5px] active:neu-shadow-inset-sm",
  {
    variants: {
      variant: {
        primary:
          "bg-neu-accent text-white neu-shadow hover:-translate-y-px hover:shadow-[12px_12px_20px_rgba(108,99,255,0.35),-12px_-12px_20px_rgba(255,255,255,0.5)] active:shadow-[inset_6px_6px_10px_rgba(0,0,0,0.2),inset_-6px_-6px_10px_rgba(255,255,255,0.2)]",
        secondary:
          "bg-neu-bg text-neu-fg neu-shadow hover:-translate-px hover:neu-shadow-hover",
      },
      size: {
        default: "h-11 px-6 py-2.5",
        sm: "h-9 px-4 text-xs",
        lg: "h-12 px-8 text-base",
      },
    },
    defaultVariants: {
      variant: "secondary",
      size: "default",
    },
  },
);

export interface NeuButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof neuButtonVariants> {
  asChild?: boolean;
}

export const NeuButton = React.forwardRef<HTMLButtonElement, NeuButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(neuButtonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    );
  },
);
NeuButton.displayName = "NeuButton";
```

- [ ] **Step 2: Create NeuIconButton**

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface NeuIconButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement> {}

export const NeuIconButton = React.forwardRef<
  HTMLButtonElement,
  NeuIconButtonProps
>(({ className, children, ...props }, ref) => {
  return (
    <button
      ref={ref}
      className={cn(
        "inline-flex h-11 w-11 items-center justify-center rounded-2xl bg-neu-bg text-neu-fg neu-shadow transition-all duration-300 ease-out hover:-translate-y-px hover:neu-shadow-hover focus-visible:outline-none focus-visible:neu-ring active:translate-y-[0.5px] active:neu-shadow-inset-sm disabled:pointer-events-none disabled:opacity-50",
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
});
NeuIconButton.displayName = "NeuIconButton";
```

- [ ] **Step 3: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/neu/button.tsx web/src/components/neu/icon-button.tsx
git commit -m "feat(neu): add button and icon-button components"
```

---

### Task 6: Create NeuInput, NeuBadge, and NeuSwitch

**Files:**
- Create: `web/src/components/neu/input.tsx`
- Create: `web/src/components/neu/badge.tsx`
- Create: `web/src/components/neu/switch.tsx`

- [ ] **Step 1: Create NeuInput**

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface NeuInputProps
  extends React.InputHTMLAttributes<HTMLInputElement> {}

export const NeuInput = React.forwardRef<HTMLInputElement, NeuInputProps>(
  ({ className, ...props }, ref) => {
    return (
      <input
        ref={ref}
        className={cn(
          "flex h-11 w-full rounded-2xl bg-neu-bg px-4 py-2 text-sm text-neu-fg placeholder:text-neu-placeholder neu-shadow-inset transition-all duration-300 ease-out focus-visible:outline-none focus-visible:neu-shadow-inset-deep focus-visible:neu-ring disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        {...props}
      />
    );
  },
);
NeuInput.displayName = "NeuInput";
```

- [ ] **Step 2: Create NeuBadge**

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface NeuBadgeProps
  extends React.HTMLAttributes<HTMLSpanElement> {}

export const NeuBadge = React.forwardRef<HTMLSpanElement, NeuBadgeProps>(
  ({ className, children, ...props }, ref) => {
    return (
      <span
        ref={ref}
        className={cn(
          "inline-flex items-center rounded-full px-3 py-1 text-xs font-semibold text-neu-muted neu-shadow-inset-sm",
          className,
        )}
        {...props}
      >
        {children}
      </span>
    );
  },
);
NeuBadge.displayName = "NeuBadge";
```

- [ ] **Step 3: Create NeuSwitch**

```tsx
import * as React from "react";
import { cn } from "@/lib/utils";

export interface NeuSwitchProps {
  checked?: boolean;
  onCheckedChange?: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
}

export function NeuSwitch({
  checked = false,
  onCheckedChange,
  disabled = false,
  className,
}: NeuSwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onCheckedChange?.(!checked)}
      className={cn(
        "relative h-7 w-12 rounded-full transition-all duration-300 ease-out focus-visible:outline-none focus-visible:neu-ring disabled:opacity-50",
        checked ? "bg-neu-accent" : "bg-neu-bg",
        checked ? "shadow-[inset_3px_3px_6px_rgba(0,0,0,0.2),inset_-3px_-3px_6px_rgba(255,255,255,0.2)]" : "neu-shadow-inset-sm",
        className,
      )}
    >
      <span
        className={cn(
          "absolute top-1 left-1 h-5 w-5 rounded-full bg-neu-bg neu-shadow-sm transition-all duration-300 ease-out",
          checked && "translate-x-5 bg-white",
        )}
      />
    </button>
  );
}
```

- [ ] **Step 4: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/neu/input.tsx web/src/components/neu/badge.tsx web/src/components/neu/switch.tsx
git commit -m "feat(neu): add input, badge, and switch components"
```

---

### Task 7: Create NeuDialog

**Files:**
- Create: `web/src/components/neu/dialog.tsx`

Build a small wrapper around Radix Dialog using the existing `@radix-ui/react-dialog` dependency.

- [ ] **Step 1: Create NeuDialog**

```tsx
import * as React from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

export const NeuDialog = DialogPrimitive.Root;
export const NeuDialogTrigger = DialogPrimitive.Trigger;
export const NeuDialogPortal = DialogPrimitive.Portal;
export const NeuDialogClose = DialogPrimitive.Close;

export const NeuDialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    className={cn(
      "fixed inset-0 z-50 bg-black/30 backdrop-blur-sm data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
      className,
    )}
    {...props}
  />
));
NeuDialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

export const NeuDialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content>
>(({ className, children, ...props }, ref) => (
  <NeuDialogPortal>
    <NeuDialogOverlay />
    <DialogPrimitive.Content
      ref={ref}
      className={cn(
        "fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 rounded-3xl bg-neu-bg p-6 neu-shadow duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]",
        className,
      )}
      {...props}
    >
      {children}
      <DialogPrimitive.Close className="absolute right-4 top-4 rounded-full p-2 text-neu-muted opacity-70 transition-opacity hover:opacity-100 focus-visible:outline-none focus-visible:neu-ring disabled:pointer-events-none">
        <X className="h-4 w-4" />
        <span className="sr-only">关闭</span>
      </DialogPrimitive.Close>
    </DialogPrimitive.Content>
  </NeuDialogPortal>
));
NeuDialogContent.displayName = DialogPrimitive.Content.displayName;

export const NeuDialogHeader = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("flex flex-col gap-1.5 text-center sm:text-left", className)} {...props} />
);
NeuDialogHeader.displayName = "NeuDialogHeader";

export const NeuDialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("font-display text-lg font-bold leading-none tracking-tight text-neu-fg", className)}
    {...props}
  />
));
NeuDialogTitle.displayName = DialogPrimitive.Title.displayName;

export const NeuDialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-neu-muted", className)}
    {...props}
  />
));
NeuDialogDescription.displayName = DialogPrimitive.Description.displayName;

export const NeuDialogFooter = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn("flex flex-col-reverse gap-2 sm:flex-row sm:justify-end", className)}
    {...props}
  />
);
NeuDialogFooter.displayName = "NeuDialogFooter";
```

- [ ] **Step 2: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/neu/dialog.tsx
git commit -m "feat(neu): add dialog component"
```

---

### Task 8: Create NeuLayout

**Files:**
- Create: `web/src/components/neu/layout.tsx`

- [ ] **Step 1: Create NeuLayout**

```tsx
import * as React from "react";
import { Link, Outlet, useLocation } from "react-router-dom";
import { Clock, Settings, Menu, Moon, Sun } from "lucide-react";
import { Sheet, SheetContent, SheetTrigger } from "@/components/ui/sheet";
import { useTheme } from "@/hooks/use-theme";
import { cn } from "@/lib/utils";
import { NeuCard } from "./card";
import { NeuIconButton } from "./icon-button";
import { NeuIconWell } from "./icon-well";

const navItems = [
  { title: "定时任务", url: "/cron-jobs", icon: Clock },
  { title: "设置", url: "/settings", icon: Settings },
];

function NavItems({ onNavigate }: { onNavigate?: () => void }) {
  const location = useLocation();
  return (
    <nav className="flex flex-col gap-3">
      {navItems.map((item) => {
        const active = location.pathname === item.url;
        return (
          <Link
            key={item.url}
            to={item.url}
            onClick={onNavigate}
            className={cn(
              "flex items-center gap-3 rounded-2xl px-4 py-3.5 text-sm font-medium text-neu-fg transition-all duration-300 ease-out focus-visible:outline-none focus-visible:neu-ring",
              active
                ? "neu-shadow-inset text-neu-accent"
                : "neu-shadow hover:-translate-y-px hover:neu-shadow-hover",
            )}
          >
            <item.icon className="h-5 w-5" />
            <span>{item.title}</span>
          </Link>
        );
      })}
    </nav>
  );
}

function ThemeToggle() {
  const buttonRef = React.useRef<HTMLButtonElement>(null);
  const theme = useTheme((state) => state.theme);
  const toggleTheme = useTheme((state) => state.toggleTheme);
  const Icon = theme === "light" ? Moon : Sun;

  return (
    <NeuIconButton
      ref={buttonRef}
      aria-label="切换主题"
      onClick={() => toggleTheme(buttonRef)}
    >
      <Icon className="h-5 w-5" />
    </NeuIconButton>
  );
}

function SidebarContent({ onNavigate }: { onNavigate?: () => void }) {
  return (
    <div className="flex h-full flex-col gap-6">
      <NeuCard className="flex items-center gap-3 p-4">
        <NeuIconWell>
          <Settings className="h-5 w-5" />
        </NeuIconWell>
        <div className="flex flex-col leading-none">
          <span className="font-display font-bold text-neu-fg">RS Template</span>
          <span className="mt-1 text-xs text-neu-muted">管理后台</span>
        </div>
      </NeuCard>
      <div>
        <div className="mb-3 px-4 text-xs font-bold uppercase tracking-wider text-neu-muted">
          导航
        </div>
        <NavItems onNavigate={onNavigate} />
      </div>
    </div>
  );
}

export default function NeuLayout() {
  const [mobileOpen, setMobileOpen] = React.useState(false);

  return (
    <div className="flex min-h-screen bg-neu-bg">
      {/* Desktop sidebar */}
      <aside className="sticky top-0 hidden h-screen w-72 flex-col p-6 md:flex">
        <SidebarContent />
      </aside>

      {/* Mobile header + sheet */}
      <div className="flex flex-1 flex-col">
        <header className="sticky top-0 z-30 flex h-20 items-center justify-between gap-4 bg-neu-bg/80 px-4 backdrop-blur-sm md:hidden">
          <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
            <SheetTrigger asChild>
              <NeuIconButton aria-label="打开菜单">
                <Menu className="h-5 w-5" />
              </NeuIconButton>
            </SheetTrigger>
            <SheetContent side="left" className="w-72 border-none bg-neu-bg p-6">
              <SidebarContent onNavigate={() => setMobileOpen(false)} />
            </SheetContent>
          </Sheet>
          <span className="font-display text-lg font-bold text-neu-fg">RS Template</span>
          <ThemeToggle />
        </header>

        <main className="flex flex-1 flex-col gap-6 p-4 md:p-8">
          <header className="hidden h-16 items-center justify-end md:flex">
            <ThemeToggle />
          </header>
          <Outlet />
        </main>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/neu/layout.tsx
git commit -m "feat(neu): add layout with sidebar, header, mobile nav"
```

---

### Task 9: Wire NeuLayout into App.tsx

**Files:**
- Modify: `web/src/App.tsx`

- [ ] **Step 1: Replace AppLayout with NeuLayout**

Replace the contents of `web/src/App.tsx` with:

```tsx
import { Navigate, Route, Routes } from "react-router-dom";
import { useInitTheme } from "@/hooks/use-theme";
import NeuLayout from "./components/neu/layout";
import CronJobsPage from "./pages/cron-jobs";
import SettingsPage from "./pages/settings";
import { Toaster } from "./components/ui/toaster";

function App() {
  useInitTheme();

  return (
    <>
      <Routes>
        <Route element={<NeuLayout />}>
          <Route path="/" element={<Navigate to="/cron-jobs" replace />} />
          <Route path="/cron-jobs" element={<CronJobsPage />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Route>
      </Routes>
      <Toaster />
    </>
  );
}

export default App;
```

- [ ] **Step 2: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/App.tsx
git commit -m "feat(neu): use NeuLayout in App"
```

---

### Task 10: Redesign Cron Jobs page

**Files:**
- Modify: `web/src/pages/cron-jobs.tsx`

Keep all query/mutation logic and interfaces; rewrite the JSX to use Neu components.

- [ ] **Step 1: Rewrite CronJobsPage**

Replace the contents of `web/src/pages/cron-jobs.tsx` with:

```tsx
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import ky from "ky";
import { Clock, Play, Pencil, Trash2 } from "lucide-react";
import { NeuCard } from "@/components/neu/card";
import { NeuButton } from "@/components/neu/button";
import { NeuIconButton } from "@/components/neu/icon-button";
import { NeuIconWell } from "@/components/neu/icon-well";
import { NeuInput } from "@/components/neu/input";
import { NeuBadge } from "@/components/neu/badge";
import { NeuSwitch } from "@/components/neu/switch";
import {
  NeuDialog,
  NeuDialogContent,
  NeuDialogDescription,
  NeuDialogFooter,
  NeuDialogHeader,
  NeuDialogTitle,
} from "@/components/neu/dialog";
import { Label } from "@/components/ui/label";
import { useToast } from "@/hooks/use-toast";

interface CronJob {
  name: string;
  title: string;
  description: string;
  expression: string;
  enabled: boolean;
  group: string;
  lastRunAt: string;
  nextRunAt: string;
  frequencySecs: number;
}

export default function CronJobsPage() {
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const [editingJob, setEditingJob] = useState<CronJob | null>(null);
  const [editForm, setEditForm] = useState({
    title: "",
    description: "",
    expression: "",
  });

  const { data: jobs, isLoading } = useQuery<CronJob[]>({
    queryKey: ["cron-jobs"],
    queryFn: async () => {
      const res = await ky
        .get("/api/cron-jobs")
        .json<{ code: string; msg: string; data: CronJob[] }>();
      return res.data ?? [];
    },
  });

  const toggleMutation = useMutation({
    mutationFn: (job: CronJob) =>
      ky.put(`/api/cron-jobs/${job.name}`, {
        json: { enabled: !job.enabled },
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron-jobs"] });
      toast({ title: "操作成功" });
    },
    onError: () => {
      toast({ title: "操作失败", variant: "destructive" });
    },
  });

  const runMutation = useMutation({
    mutationFn: (name: string) => ky.post(`/api/cron-jobs/${name}/run`),
    onSuccess: () => {
      toast({ title: "任务已触发执行" });
    },
    onError: () => {
      toast({ title: "执行失败", variant: "destructive" });
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ name, data }: { name: string; data: any }) =>
      ky.put(`/api/cron-jobs/${name}`, { json: data }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron-jobs"] });
      setEditingJob(null);
      toast({ title: "更新成功" });
    },
    onError: () => {
      toast({ title: "更新失败", variant: "destructive" });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (name: string) => ky.delete(`/api/cron-jobs/${name}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["cron-jobs"] });
      toast({ title: "删除成功" });
    },
    onError: () => {
      toast({ title: "删除失败", variant: "destructive" });
    },
  });

  const handleEdit = (job: CronJob) => {
    setEditingJob(job);
    setEditForm({
      title: job.title,
      description: job.description,
      expression: job.expression,
    });
  };

  const handleSave = () => {
    if (editingJob) {
      updateMutation.mutate({ name: editingJob.name, data: editForm });
    }
  };

  const handleDelete = (name: string) => {
    if (confirm("确定要删除这个任务吗？")) {
      deleteMutation.mutate(name);
    }
  };

  const formatDate = (dateStr: string) => {
    if (!dateStr || dateStr === "1970-01-01T00:00:00+00:00") return "-";
    return new Date(dateStr).toLocaleString("zh-CN");
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-20 text-neu-muted">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-neu-accent border-t-transparent" />
        <span className="ml-3">加载中...</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="font-display text-3xl font-extrabold tracking-tight text-neu-fg md:text-4xl">
          定时任务
        </h1>
        <p className="mt-1 text-neu-muted">管理系统定时任务</p>
      </div>

      {jobs && jobs.length > 0 ? (
        <div className="grid gap-4">
          {jobs.map((job) => (
            <NeuCard
              key={job.name}
              hover
              className="flex flex-col gap-4 p-5 md:flex-row md:items-center md:justify-between"
            >
              <div className="flex items-start gap-4">
                <NeuIconWell>
                  <Clock className="h-5 w-5" />
                </NeuIconWell>
                <div className="min-w-0">
                  <h3 className="font-display font-bold text-neu-fg">{job.name}</h3>
                  <p className="text-sm text-neu-muted">{job.title}</p>
                  <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-neu-muted">
                    <NeuBadge>{job.expression}</NeuBadge>
                    <span>下次执行 {formatDate(job.nextRunAt)}</span>
                  </div>
                </div>
              </div>

              <div className="flex items-center justify-end gap-3">
                <NeuSwitch
                  checked={job.enabled}
                  onCheckedChange={() => toggleMutation.mutate(job)}
                />
                <NeuIconButton
                  onClick={() => runMutation.mutate(job.name)}
                  title="立即执行"
                >
                  <Play className="h-4 w-4" />
                </NeuIconButton>
                <NeuIconButton
                  onClick={() => handleEdit(job)}
                  title="编辑"
                >
                  <Pencil className="h-4 w-4" />
                </NeuIconButton>
                <NeuIconButton
                  className="text-red-500"
                  onClick={() => handleDelete(job.name)}
                  title="删除"
                >
                  <Trash2 className="h-4 w-4" />
                </NeuIconButton>
              </div>
            </NeuCard>
          ))}
        </div>
      ) : (
        <NeuCard className="py-16 text-center">
          <p className="text-neu-muted">暂无定时任务</p>
        </NeuCard>
      )}

      <NeuDialog open={!!editingJob} onOpenChange={() => setEditingJob(null)}>
        <NeuDialogContent>
          <NeuDialogHeader>
            <NeuDialogTitle>编辑任务</NeuDialogTitle>
            <NeuDialogDescription>修改定时任务的基本信息</NeuDialogDescription>
          </NeuDialogHeader>
          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="title">标题</Label>
              <NeuInput
                id="title"
                value={editForm.title}
                onChange={(e) =>
                  setEditForm({ ...editForm, title: e.target.value })
                }
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="description">描述</Label>
              <NeuInput
                id="description"
                value={editForm.description}
                onChange={(e) =>
                  setEditForm({ ...editForm, description: e.target.value })
                }
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="expression">Cron 表达式</Label>
              <NeuInput
                id="expression"
                value={editForm.expression}
                onChange={(e) =>
                  setEditForm({ ...editForm, expression: e.target.value })
                }
              />
            </div>
          </div>
          <NeuDialogFooter>
            <NeuButton
              variant="secondary"
              onClick={() => setEditingJob(null)}
            >
              取消
            </NeuButton>
            <NeuButton variant="primary" onClick={handleSave}>
              保存
            </NeuButton>
          </NeuDialogFooter>
        </NeuDialogContent>
      </NeuDialog>
    </div>
  );
}
```

- [ ] **Step 2: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/pages/cron-jobs.tsx
git commit -m "feat(neu): redesign cron jobs page with neu components"
```

---

### Task 11: Redesign Settings page

**Files:**
- Modify: `web/src/pages/settings.tsx`

- [ ] **Step 1: Rewrite SettingsPage**

Replace the contents of `web/src/pages/settings.tsx` with:

```tsx
import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import ky from "ky";
import { Settings, Pencil } from "lucide-react";
import { NeuCard } from "@/components/neu/card";
import { NeuButton } from "@/components/neu/button";
import { NeuIconButton } from "@/components/neu/icon-button";
import { NeuIconWell } from "@/components/neu/icon-well";
import { NeuInput } from "@/components/neu/input";
import { NeuBadge } from "@/components/neu/badge";
import {
  NeuDialog,
  NeuDialogContent,
  NeuDialogDescription,
  NeuDialogFooter,
  NeuDialogHeader,
  NeuDialogTitle,
} from "@/components/neu/dialog";
import { Label } from "@/components/ui/label";
import { useToast } from "@/hooks/use-toast";

interface Setting {
  key: string;
  value: string;
  type: string;
  updatedAt: string;
}

export default function SettingsPage() {
  const { toast } = useToast();
  const queryClient = useQueryClient();
  const [editingSetting, setEditingSetting] = useState<Setting | null>(null);
  const [editValue, setEditValue] = useState("");

  const { data: settings, isLoading } = useQuery<Setting[]>({
    queryKey: ["settings"],
    queryFn: async () => {
      const res = await ky
        .get("/api/settings")
        .json<{ code: string; msg: string; data: Setting[] }>();
      return res.data ?? [];
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ key, value }: { key: string; value: string }) =>
      ky.put(`/api/settings/${key}`, { json: { value } }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      setEditingSetting(null);
      toast({ title: "更新成功" });
    },
    onError: () => {
      toast({ title: "更新失败", variant: "destructive" });
    },
  });

  const handleEdit = (setting: Setting) => {
    setEditingSetting(setting);
    setEditValue(setting.value);
  };

  const handleSave = () => {
    if (editingSetting) {
      updateMutation.mutate({ key: editingSetting.key, value: editValue });
    }
  };

  const formatDate = (dateStr: string) => {
    if (!dateStr) return "-";
    return new Date(dateStr).toLocaleString("zh-CN");
  };

  const typeColors: Record<string, string> = {
    String: "text-neu-accent",
    Int: "text-neu-success",
    Float: "text-neu-success",
    Bool: "text-neu-muted",
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-20 text-neu-muted">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-neu-accent border-t-transparent" />
        <span className="ml-3">加载中...</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="font-display text-3xl font-extrabold tracking-tight text-neu-fg md:text-4xl">
          设置
        </h1>
        <p className="mt-1 text-neu-muted">管理系统配置项</p>
      </div>

      {settings && settings.length > 0 ? (
        <div className="grid gap-4">
          {settings.map((setting) => (
            <NeuCard
              key={setting.key}
              hover
              className="flex flex-col gap-4 p-5 md:flex-row md:items-center md:justify-between"
            >
              <div className="flex items-start gap-4 min-w-0">
                <NeuIconWell>
                  <Settings className="h-5 w-5" />
                </NeuIconWell>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="font-display font-bold text-neu-fg">
                      {setting.key}
                    </h3>
                    <NeuBadge className={typeColors[setting.type] ?? "text-neu-muted"}>
                      {setting.type}
                    </NeuBadge>
                  </div>
                  <p className="mt-1 truncate text-sm text-neu-fg">{setting.value}</p>
                  <p className="mt-1 text-xs text-neu-muted">
                    更新于 {formatDate(setting.updatedAt)}
                  </p>
                </div>
              </div>

              <div className="flex items-center justify-end">
                <NeuIconButton
                  onClick={() => handleEdit(setting)}
                  title="编辑"
                >
                  <Pencil className="h-4 w-4" />
                </NeuIconButton>
              </div>
            </NeuCard>
          ))}
        </div>
      ) : (
        <NeuCard className="py-16 text-center">
          <p className="text-neu-muted">暂无设置项</p>
        </NeuCard>
      )}

      <NeuDialog
        open={!!editingSetting}
        onOpenChange={() => setEditingSetting(null)}
      >
        <NeuDialogContent>
          <NeuDialogHeader>
            <NeuDialogTitle>编辑设置</NeuDialogTitle>
            <NeuDialogDescription>
              修改 {editingSetting?.key} 的值
            </NeuDialogDescription>
          </NeuDialogHeader>
          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="value">值</Label>
              <NeuInput
                id="value"
                value={editValue}
                onChange={(e) => setEditValue(e.target.value)}
              />
            </div>
          </div>
          <NeuDialogFooter>
            <NeuButton
              variant="secondary"
              onClick={() => setEditingSetting(null)}
            >
              取消
            </NeuButton>
            <NeuButton variant="primary" onClick={handleSave}>
              保存
            </NeuButton>
          </NeuDialogFooter>
        </NeuDialogContent>
      </NeuDialog>
    </div>
  );
}
```

- [ ] **Step 2: Verify build**

Run: `cd web && pnpm build`
Expected: build succeeds.

- [ ] **Step 3: Commit**

```bash
git add web/src/pages/settings.tsx
git commit -m "feat(neu): redesign settings page with neu components"
```

---

### Task 12: Final verification

**Files:**
- None (verification only)

- [ ] **Step 1: Run frontend build**

Run: `cd web && pnpm build`
Expected: `dist/` generated with no errors.

- [ ] **Step 2: Run frontend lint**

Run: `cd web && pnpm lint`
Expected: no errors or warnings.

- [ ] **Step 3: Run Rust release build**

Run: `cargo build --release`
Expected: release binary builds successfully, confirming `rust-embed` packages `web/dist`.

- [ ] **Step 4: Manual spot-check list**

Open the app (via `cargo run` and the browser) and verify:
- [ ] Light mode shows cool-grey background with extruded cards.
- [ ] Dark mode shows dark-grey background with preserved shadow physics.
- [ ] Focus rings are visible on all buttons, inputs, links, and switch.
- [ ] Mobile sidebar opens/closes via hamburger.
- [ ] Cron job toggle/run/edit/delete still work.
- [ ] Settings edit still works.

- [ ] **Step 5: Commit any remaining fixes**

If any fixes were required, commit them. Then the plan is complete.

---

## Plan Self-Review

**Spec coverage:**
- Design tokens → Task 2, Task 3.
- Typography/fonts → Task 1, Task 3.
- Shadows/effects → Task 2 (CSS utilities).
- Component architecture → Tasks 4–7.
- Layout → Task 8.
- Page redesign → Tasks 10–11.
- Animations → Task 2 (float keyframe), Task 3 (animation), hover/active transitions in components.
- Accessibility → focus rings in utilities, 44px icon buttons, keyboard-navigable links.
- Responsive → Task 8 mobile sheet, page grids stack on mobile.
- Migration/testing → Tasks 9, 12.

**Placeholder scan:**
- No TBD/TODO. All code blocks contain complete implementations. No vague instructions.

**Type consistency:**
- All components use `cn()` from `@/lib/utils`.
- `NeuButton` uses CVA consistent with existing `button.tsx`.
- `NeuSwitch` interface matches usage in pages.
- `NeuDialog` re-uses Radix Dialog primitives already in `package.json`.
