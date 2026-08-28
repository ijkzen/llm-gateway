# Multi-Theme Toggle Design

## Overview

Add Light/Dark theme toggle to the frontend with a circular reveal animation using the View Transition API. The toggle button is placed in the top-right corner of the header.

## Decisions

- **Theme scope**: Light and Dark only (no additional presets)
- **Persistence**: localStorage via Zustand `persist` middleware
- **Toggle style**: Icon button (Sun/Moon) using lucide-react icons
- **Animation**: Circular reveal using `document.startViewTransition()` + CSS `clip-path`

## Architecture

### Files

| File | Action | Purpose |
|------|--------|---------|
| `web/src/hooks/use-theme.ts` | Create | Zustand store for theme state |
| `web/src/components/theme-toggle.tsx` | Create | Toggle button component |
| `web/src/components/layout.tsx` | Modify | Add ThemeToggle to header |
| `web/src/App.tsx` | Modify | Call useInitTheme on mount |
| `web/index.html` | Modify | Add anti-FOUT inline script |

### Theme Store (`use-theme.ts`)

Zustand store with `persist` middleware:

```ts
interface ThemeStore {
  theme: "light" | "dark";
  toggleTheme: (buttonRef: React.RefObject<HTMLButtonElement>) => void;
}
```

- **State**: `theme: "light" | "dark"`, default `"light"`
- **Actions**: `toggleTheme(buttonRef)` — flips between light and dark, accepts button ref for animation origin
- **Persist**: Zustand `persist` middleware, storage key `"rs-template-theme"`, stored in localStorage
- **Side effects**: Store subscribes to state changes and syncs `"dark"` class on `document.documentElement`
- **Initialization**: `useInitTheme()` hook reads current store value and applies class on mount

### Toggle Button (`theme-toggle.tsx`)

- **Icon**: `Sun` and `Moon` from `lucide-react`, conditionally rendered based on current theme
- **Click handler**: Calls `useTheme().toggleTheme(buttonRef)` with a ref to the button element
- **Styling**: shadcn `Button` with `variant="ghost"` and `size="icon"` — consistent with `SidebarTrigger`
- **Position**: Placed in header, wrapped in a `div.ml-auto` to push to the right side

### Layout Changes (`layout.tsx`)

Header layout becomes:

```
[SidebarTrigger] [Separator] [div.ml-auto > ThemeToggle]
```

The existing header has `flex` layout. A wrapper `div` with `ml-auto` before ThemeToggle pushes it to the right.

### Anti-FOUT Script (`index.html`)

Inline `<script>` in `<head>` that:

1. Reads `"rs-template-theme"` from localStorage
2. Parses the JSON to extract the theme value
3. If theme is `"dark"`, adds `"dark"` class to `document.documentElement`

This executes before React hydration, ensuring the first paint uses the correct theme.

### Circular Reveal Animation

Using the View Transition API for a smooth circular reveal effect:

1. **Trigger**: `toggleTheme(buttonRef)` receives a React ref to the button. At click time, before calling `startViewTransition()`, capture the button's center coordinates via `buttonRef.current.getBoundingClientRect()` and set them as CSS custom properties `--x` and `--y` on `document.documentElement`
2. **Position**: Coordinates are captured synchronously at click time to ensure the animation origin matches the button's current position

```css
::view-transition-old(root) {
  animation: none;
}

::view-transition-new(root) {
  animation: reveal-circle 0.4s ease-out;
}

@keyframes reveal-circle {
  from {
    clip-path: circle(0% at var(--x) var(--y));
  }
  to {
    clip-path: circle(150% at var(--x) var(--y));
  }
}
```

4. **Fallback**: Browsers without View Transition API support get an instant switch (no animation, functionality unaffected)

## Error Handling

- **localStorage unavailable**: Zustand persist handles this gracefully — falls back to in-memory state, theme resets to default on refresh
- **View Transition API unsupported**: `document.startViewTransition()` check — if undefined, apply theme change directly without animation

## Testing

- Manual verification: toggle between themes, refresh page to confirm persistence, verify no FOUT on refresh
- Verify animation plays smoothly from button position
- Verify fallback works in browsers without View Transition API support
