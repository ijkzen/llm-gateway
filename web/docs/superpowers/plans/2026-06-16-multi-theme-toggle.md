# Multi-Theme Toggle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Light/Dark theme toggle with circular reveal animation to the frontend header.

**Architecture:** Zustand store with `persist` middleware manages theme state and syncs to `document.documentElement` class. Toggle button in header triggers theme switch with View Transition API animation.

**Tech Stack:** React 19, Zustand 5, Tailwind CSS 3, shadcn/ui, lucide-react, View Transition API

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `web/src/hooks/use-theme.ts` | Create | Zustand store for theme state |
| `web/src/components/theme-toggle.tsx` | Create | Toggle button component |
| `web/src/components/layout.tsx` | Modify | Add ThemeToggle to header |
| `web/src/App.tsx` | Modify | Call useInitTheme on mount |
| `web/index.html` | Modify | Add anti-FOUT inline script |
| `web/src/index.css` | Modify | Add View Transition animation CSS |

---

### Task 1: Theme Store

**Files:**
- Create: `web/src/hooks/use-theme.ts`

- [ ] **Step 1: Create theme store**

```ts
import { create } from "zustand";
import { persist } from "zustand/middleware";

interface ThemeStore {
  theme: "light" | "dark";
  toggleTheme: (buttonRef: React.RefObject<HTMLButtonElement>) => void;
}

export const useTheme = create<ThemeStore>()(
  persist(
    (set, get) => ({
      theme: "light",
      toggleTheme: (buttonRef) => {
        const newTheme = get().theme === "light" ? "dark" : "light";

        // Capture button position for animation
        if (buttonRef.current) {
          const rect = buttonRef.current.getBoundingClientRect();
          const x = rect.left + rect.width / 2;
          const y = rect.top + rect.height / 2;
          document.documentElement.style.setProperty("--x", `${x}px`);
          document.documentElement.style.setProperty("--y", `${y}px`);
        }

        // Apply theme with View Transition API if supported
        if (document.startViewTransition) {
          document.startViewTransition(() => {
            set({ theme: newTheme });
            document.documentElement.classList.toggle("dark", newTheme === "dark");
          });
        } else {
          set({ theme: newTheme });
          document.documentElement.classList.toggle("dark", newTheme === "dark");
        }
      },
    }),
    {
      name: "rs-template-theme",
    }
  )
);

export function useInitTheme() {
  const theme = useTheme((state) => state.theme);
  document.documentElement.classList.toggle("dark", theme === "dark");
}
```

- [ ] **Step 2: Verify store compiles**

Run: `cd web && pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add web/src/hooks/use-theme.ts
git commit -m "feat: add theme store with persist middleware"
```

---

### Task 2: Toggle Button Component

**Files:**
- Create: `web/src/components/theme-toggle.tsx`

- [ ] **Step 1: Create theme toggle component**

```tsx
import { useRef } from "react";
import { Sun, Moon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTheme } from "@/hooks/use-theme";

export function ThemeToggle() {
  const buttonRef = useRef<HTMLButtonElement>(null);
  const theme = useTheme((state) => state.theme);
  const toggleTheme = useTheme((state) => state.toggleTheme);

  return (
    <Button
      ref={buttonRef}
      variant="ghost"
      size="icon"
      onClick={() => toggleTheme(buttonRef)}
    >
      {theme === "light" ? <Moon className="size-4" /> : <Sun className="size-4" />}
    </Button>
  );
}
```

- [ ] **Step 2: Verify component compiles**

Run: `cd web && pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 3: Commit**

```bash
git add web/src/components/theme-toggle.tsx
git commit -m "feat: add theme toggle button component"
```

---

### Task 3: Layout Integration

**Files:**
- Modify: `web/src/components/layout.tsx`

- [ ] **Step 1: Import ThemeToggle**

Add import at top of file:
```tsx
import { ThemeToggle } from "@/components/theme-toggle";
```

- [ ] **Step 2: Add ThemeToggle to header**

Replace the header section (lines 79-81):
```tsx
<header className="flex h-16 shrink-0 items-center gap-2 border-b px-4">
  <SidebarTrigger className="-ml-1" />
  <Separator orientation="vertical" className="mr-2 h-4" />
  <div className="ml-auto">
    <ThemeToggle />
  </div>
</header>
```

- [ ] **Step 3: Verify layout compiles**

Run: `cd web && pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add web/src/components/layout.tsx
git commit -m "feat: add theme toggle to header layout"
```

---

### Task 4: App Initialization

**Files:**
- Modify: `web/src/App.tsx`

- [ ] **Step 1: Import useInitTheme**

Add import at top of file:
```tsx
import { useInitTheme } from "@/hooks/use-theme";
```

- [ ] **Step 2: Call useInitTheme in App component**

Add hook call inside App function, before the return:
```tsx
function App() {
  useInitTheme();

  return (
    // ... existing JSX
  );
}
```

- [ ] **Step 3: Verify App compiles**

Run: `cd web && pnpm tsc --noEmit`
Expected: No errors

- [ ] **Step 4: Commit**

```bash
git add web/src/App.tsx
git commit -m "feat: initialize theme on app mount"
```

---

### Task 5: Anti-FOUT Script

**Files:**
- Modify: `web/index.html`

- [ ] **Step 1: Add inline script to head**

Add before closing `</head>` tag:
```html
<script>
  (function() {
    try {
      const stored = localStorage.getItem("rs-template-theme");
      if (stored) {
        const parsed = JSON.parse(stored);
        if (parsed.state && parsed.state.theme === "dark") {
          document.documentElement.classList.add("dark");
        }
      }
    } catch (e) {}
  })();
</script>
```

- [ ] **Step 2: Verify HTML is valid**

Run: `cd web && pnpm build`
Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add web/index.html
git commit -m "feat: add anti-FOUT script for theme persistence"
```

---

### Task 6: Circular Reveal Animation CSS

**Files:**
- Modify: `web/src/index.css`

- [ ] **Step 1: Add View Transition CSS**

Add after the existing `@layer base` block:
```css
/* View Transition animation for theme toggle */
::view-transition-old(root) {
  animation: none;
}

::view-transition-new(root) {
  animation: reveal-circle 0.4s ease-out;
}

@keyframes reveal-circle {
  from {
    clip-path: circle(0% at var(--x, 50%) var(--y, 50%));
  }
  to {
    clip-path: circle(150% at var(--x, 50%) var(--y, 50%));
  }
}
```

- [ ] **Step 2: Verify CSS compiles**

Run: `cd web && pnpm build`
Expected: Build succeeds

- [ ] **Step 3: Commit**

```bash
git add web/src/index.css
git commit -m "feat: add circular reveal animation CSS"
```

---

### Task 7: Integration Test

- [ ] **Step 1: Start dev server**

Run: `cd web && pnpm dev`
Expected: Server starts on localhost

- [ ] **Step 2: Verify theme toggle works**

1. Open browser to dev server URL
2. Click theme toggle button in top-right corner
3. Verify theme switches from light to dark with circular reveal animation
4. Click again to switch back to light
5. Refresh page — theme should persist

- [ ] **Step 3: Verify no FOUT**

1. Set theme to dark
2. Refresh page
3. Verify page loads in dark mode immediately (no flash of light theme)

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat: complete multi-theme toggle implementation"
```
