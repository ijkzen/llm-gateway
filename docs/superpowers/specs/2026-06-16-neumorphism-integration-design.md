# Neumorphism (Soft UI) Design System Integration

## Goal

Apply a cohesive Neumorphism (Soft UI) visual system to the existing React admin frontend (`web/`) while keeping all business logic, API contracts, and backend code unchanged. The result should feel tactile, modern, and physically grounded, with the cool-grey monochromatic palette and dual-shadow physics described in the provided design system.

## Scope & Decisions

- **Scope**: Redesign the existing admin pages (Cron Jobs, Settings) and the application shell (sidebar/header).
- **Approach**: Token-first full integration.
  - Centralize design tokens in CSS variables and Tailwind config.
  - Create a parallel `Neu*` component layer in `web/src/components/neu/`.
  - Leave existing shadcn/ui primitives untouched for safety.
- **Theme**: Extend Neumorphism to both light and dark modes. The provided system is light-only; we will derive a dark-surface variant that preserves the same shadow physics.
- **Layout**: Keep the existing sidebar navigation but restyle it with Neumorphic extruded/inset surfaces.
- **Fonts**: Load `Plus Jakarta Sans` for display headings and `DM Sans` for body text via Google Fonts.

## Design Tokens

### Colors

| Token | Light | Dark | Usage |
|-------|-------|------|-------|
| `--neu-bg` | `#E0E5EC` | `#232931` | Page/card background |
| `--neu-fg` | `#3D4852` | `#E0E5EC` | Primary text |
| `--neu-muted` | `#6B7280` | `#9CA3AF` | Secondary text |
| `--neu-accent` | `#6C63FF` | `#8B84FF` | CTAs, focus, active switch |
| `--neu-accent-light` | `#8B84FF` | `#A5A0FF` | Gradients/hover accents |
| `--neu-success` | `#38B2AC` | `#4FD1C5` | Positive indicators |
| `--neu-placeholder` | `#A0AEC0` | `#6B7280` | Input placeholders |

### Shadows

All shadows use RGBA for smooth blending.

| Token | Light | Dark |
|-------|-------|------|
| `--neu-shadow-light` | `rgba(255,255,255,0.55)` | `rgba(255,255,255,0.08)` |
| `--neu-shadow-dark` | `rgba(163,177,198,0.65)` | `rgba(0,0,0,0.55)` |

Standard extruded shadow:
```css
box-shadow: 9px 9px 16px var(--neu-shadow-dark), -9px -9px 16px var(--neu-shadow-light);
```

Inset deep shadow (inputs, icon wells):
```css
box-shadow: inset 10px 10px 20px var(--neu-shadow-dark), inset -10px -10px 20px var(--neu-shadow-light);
```

### Radii

- Cards / page containers: `32px`
- Buttons / inputs: `16px`
- Inner wells / icon holders: `12px`
- Pills / badges / switch tracks: `9999px`

### Typography

- **Display**: `Plus Jakarta Sans`, weights 700–800, `tracking-tight`.
- **Body**: `DM Sans`, weights 400–700.

## Component Architecture

New components live in `web/src/components/neu/`.

### Primitives

- `NeuButton` — extruded button with primary/secondary/icon variants; hover lift and active press.
- `NeuIconButton` — square extruded button for actions (run, edit, delete).
- `NeuCard` — extruded rounded container (`32px`).
- `NeuInput` — inset input that goes inset-deep on focus, with accent focus ring.
- `NeuSwitch` — pill track with sliding thumb; on-state uses accent.
- `NeuBadge` — inset pill for cron expressions/type tags.
- `NeuIconWell` — inset-deep circular/rounded container for icons.
- `NeuDialog` — extruded dialog overlay with inset-deep body inputs.

### Layout

- `NeuLayout` — replaces the existing `AppLayout`.
  - `NeuSidebar` — extruded brand card, nav group label, `NeuSidebarItem` with active inset state.
  - `NeuHeader` — extruded sticky header with page title and theme toggle icon button.
  - `NeuMobileNav` — hamburger sheet for screens below `md`.

## Page Redesign

### Cron Jobs Page

- Replace the dense table with a card list.
- Each job renders as a `NeuCard` containing:
  - `NeuIconWell` with a clock/play icon.
  - Job name (Plus Jakarta Sans, bold).
  - Description, cron expression badge, and next-run time on one line.
  - Right side: `NeuSwitch` for enable/disable, `NeuIconButton` for run/edit/delete.
- Empty state inside an extruded card with centered muted text.
- Edit dialog uses `NeuDialog` + `NeuInput` + `NeuButton`.

### Settings Page

- Same card-list pattern as Cron Jobs.
- Each setting card shows:
  - `NeuIconWell` with a settings icon.
  - Key name and truncated value.
  - Type badge (`String`, `Int`, `Float`, `Bool`).
  - Updated-at time.
  - Edit icon button.
- Edit dialog uses `NeuDialog` + `NeuInput`.

## Animations & Accessibility

- **Transitions**: `transition-all duration-300 ease-out` on transform and box-shadow for all interactive elements.
- **Hover**: `-translate-y-1` + enhanced shadow for cards and buttons.
- **Active**: `translate-y-[0.5px]` + inset shadow for buttons.
- **Ambient**: `float` keyframe (3s ease-in-out infinite) on decorative background circles.
- **Focus**: `ring-2 ring-[var(--neu-accent)] ring-offset-2 ring-offset-[var(--neu-bg)]` on every focusable element.
- **Touch targets**: Minimum 44×44px (icon buttons are 44px).
- **Responsive**:
  - Sidebar collapses to a sheet on mobile.
  - Card lists stack vertically.
  - Page title scales from `text-xl`/`text-2xl` to `text-3xl`/`text-4xl`.

## Migration Plan

1. Add design tokens to `web/src/index.css`.
2. Extend `web/tailwind.config.ts` with Neu colors, shadows, fonts, and animation keyframes.
3. Add Google Fonts import to `web/index.html` with `display=swap`.
4. Create `Neu*` components in `web/src/components/neu/`.
5. Create `NeuLayout` and update `web/src/App.tsx` to use it.
6. Rewrite `web/src/pages/cron-jobs.tsx` and `web/src/pages/settings.tsx` to use Neu components while preserving all query/mutation logic.
7. Verify the theme toggle still applies `.dark` and that dark tokens render correctly.
8. Run `pnpm build`, `pnpm lint`, and `cargo build --release`.

## Testing & Verification

- `cd web && pnpm build` passes.
- `cd web && pnpm lint` passes.
- `cargo build --release` passes (confirms `web/dist` is packaged by `rust-embed`).
- Manual spot-checks:
  - Light and dark modes render Neumorphic surfaces.
  - Focus rings are visible on all interactive elements.
  - Mobile sidebar opens/closes.
  - Cron job toggle/run/edit/delete still work.
  - Settings edit still works.

## Out of Scope

- No backend changes.
- No new pages or features beyond the visual redesign.
- No changes to existing shadcn/ui files in `web/src/components/ui/`.
- No new API endpoints.

## Mockup

A browser mockup is saved in the brainstorming session at `.superpowers/brainstorm/18803-1781648937/content/admin-mockup.html` and was served during review.
