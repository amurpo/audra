# Audra Landing Page Design System

> Category: Developer Tools & Audio Software
> Desktop music player. Dark brutalist interface, native GNOME/GTK4 aesthetic, minimal and focused.

## 1. Visual Theme & Atmosphere

Audra's landing page embodies a **dark brutalist aesthetic** designed for a developer and Linux-centric audience. The design communicates confidence and focus through stark contrast, generous whitespace, and intentional typography. Built to reflect the native GNOME/GTK4 sensibility of the app itself — clean, unadorned, and purposeful.

The visual language prioritizes **readability and clarity** over decoration. A single accent color (`#4f55f1` — bright electric blue) punctuates the design, appearing only on interactive elements and key value propositions. The palette is deliberately restrained: near-black (`#191c1f`), pure white (`#ffffff`), and grey borders (`#2a2f37`) create depth through contrast alone, never through shadows or gradients.

**Key Characteristics:**
- Near-black (`#191c1f`) + white binary — stark, high-contrast surfaces
- Single accent (`#4f55f1`) used only for buttons, key numbers, and focal points
- Inter sans-serif for all typography — clean, neutral, universally readable
- Zero shadows — depth through color contrast only
- Borders as the primary visual separator (1px, `#2a2f37`)
- Generous whitespace and breathing room between sections
- Native desktop aesthetic — no web design ornament

## 2. Color Palette & Roles

### Primary
- **Audra Dark** (`#191c1f`): Background, surface color, near-black text base
- **Pure White** (`#ffffff`): Primary text, light interactive states
- **Border Grey** (`#2a2f37`): Section dividers, subtle borders, hover states

### Interactive & Brand
- **Audra Blue** (`#4f55f1`): Primary CTA, accent, focus states, numbered steps
- **Blue Hover** (`rgba(79, 85, 241, 0.1)`): Ghost button background on hover

### Semantic
- **Muted Text** (`opacity-75` / `opacity-85`): Secondary content, body text hierarchy
- **Disabled State** (`opacity-60`): Footer links, muted interactions

## 3. Typography Rules

### Font Family
- **Primary**: `Inter`, `-apple-system`, `BlinkMacSystemFont`, `'Segoe UI'`, `system-ui`, `sans-serif`
- **Fallback stack**: System sans-serif across all platforms

### Hierarchy

| Role | Size | Weight | Line Height | Letter Spacing | Usage |
|------|------|--------|-------------|----------------|-------|
| Page Title (H1) | 3rem–3.5rem (md: 4rem–4.5rem) | 700 | 1.2 | -0.02em | Hero headline "Audra" |
| Section Heading (H2) | 1.875rem–2.25rem | 700 | 1.3 | normal | "Overview", "Features", "Installation" |
| Card Title (H3) | 1.25rem–1.5rem | 600 | 1.4 | normal | Feature cards, step titles |
| Body Large | 1.125rem | 400 | 1.5 | 0 | Hero description, introductions |
| Body | 1rem | 400 | 1.5 | 0 | Standard reading text |
| Body Small | 0.875rem | 400 | 1.6 | normal | Captions, footer, metadata |
| Nav/Button | 1rem | 500 | 1.5 | 0 | Navigation links, button labels |

### Principles
- **Weight 700 for headings** — authority through weight + size, not tracking
- **400 for body** — maximum readability, high contrast on dark background
- **No aggressive tracking** — Inter's metrics are already balanced
- **Line height 1.5–1.6 for body** — ample breathing room for dark-on-light contrast

## 4. Component Styling

### Buttons

**Primary CTA (Filled Blue)**
- Background: `#4f55f1`
- Text: `#ffffff`
- Padding: `1rem 2rem` (16px 32px)
- Radius: `0.5rem` (8px)
- Border: none
- Hover: `opacity: 0.85`
- Focus: ring (browser default or custom focus-visible)

**Secondary (Outlined)**
- Background: transparent
- Text: `#4f55f1`
- Border: `1px solid #4f55f1`
- Padding: `1rem 2rem`
- Radius: `0.5rem`
- Hover: `backgroundColor: rgba(79, 85, 241, 0.1)`
- Focus: ring

**Ghost Link**
- Background: transparent
- Text: `#ffffff` or `opacity-60` (footer links)
- Border: none
- Hover: `opacity-100` (fade from `opacity-60`)

### Cards & Containers
- Radius: `0.5rem` (8px) — subtle, not rounded
- Border: `1px solid #2a2f37` — restrained visual separator
- Hover: `border-color: #4a5263` — slight lightening on interaction
- Padding: `1.5rem` (24px) — breathing room inside cards
- Background: `#191c1f` (inherit)

### Navigation
- Position: `sticky`, `top: 0`
- Backdrop: `blur(8px)` with `rgba(25, 28, 31, 0.95)`
- Border: `1px solid #2a2f37`
- Links + buttons in flex row, right-aligned

### Code Blocks
- Background: `rgba(0, 0, 0, 0.4)` (slightly darker than page)
- Border: none
- Padding: `1rem` (16px)
- Radius: `0.5rem`
- Text: `#60a5fa` (blue-400 for code, not the accent)
- Font: monospace
- Overflow-x: auto (horizontal scroll on narrow)

## 5. Layout Principles

### Grid System
- **Mobile (< 768px)**: Single column, full width minus 24px padding
- **Tablet (768px–1024px)**: 2-column grid on features, single on other sections
- **Desktop (≥1024px)**: 3-column grid on features, 2-column on installation

### Spacing System
- Base unit: 4px (Tailwind default)
- Key multiples: 8px, 16px, 24px, 32px, 48px, 64px, 96px
- Section vertical spacing: `py-24` (96px) default, `py-32` (128px) on hero
- Section borders: `border-b border-gray-900` (adds 1px, visual separator)
- Horizontal container padding: `px-6` (24px) across all breakpoints

### Responsive Breakpoints
| Breakpoint | Width | Key Changes |
|---|---|---|
| Mobile | < 768px | Single column, stacked buttons, compact padding |
| Tablet | 768px–1024px | 2-column grids, side-by-side layouts where sensible |
| Desktop | ≥ 1024px | 3-column grids, full layout potential |

## 6. Depth & Elevation

| Level | Treatment | Use |
|---|---|---|
| Surface (L0) | `background-color: #191c1f` | Page background, default state |
| Card (L1) | `background-color: #191c1f + border: 1px #2a2f37` | Feature cards, installation sections |
| Interactive (L2) | `background-color: #4f55f1` | Primary buttons, focused elements, numbered steps |
| Hover (L3) | `backgroundColor: rgba(79, 85, 241, 0.1)` on outline buttons | Ghost button feedback |

**Shadow Philosophy**: Audra uses **zero shadows**. Depth comes entirely from border contrast (`#2a2f37`) and background color shifts (`#4f55f1` on interactive elements).

## 7. Feature Grid (9 Cards)

Each feature card follows this structure:
- **Title** (H3, `font-semibold`, `text-xl`)
- **Description** (body text, `opacity-75`)
- **Container**: `p-6 rounded-lg border border-gray-800 hover:border-gray-700`
- **Interaction**: Border lightens on hover

Grid layout:
- Mobile: 1 column
- Tablet: 2 columns
- Desktop: 3 columns (`grid-cols-3`)

## 8. Steps Section (5-Step Flow)

Numbered steps use:
- **Number badge**: `#4f55f1` background, white text, `w-12 h-12`, `rounded-lg`, centered text
- **Title** (H3, `font-semibold`, `text-xl`)
- **Description** (body, `opacity-75`)
- **Layout**: Flexbox row, badge flex-shrink-0, text takes remaining space
- **Spacing**: `gap-6` between badge and text column

## 9. Installation Hybrid (Prose + Code)

**Prose Column (Left, Tablet+)**
- Max-width prose styling
- Paragraphs separated by `mb-6`
- Normal body text hierarchy

**Code Column (Right, Tablet+)**
- Monospace code blocks
- Three command sets (From Source, Debian, Fedora)
- Each block: label + code
- Label: `opacity-60`, `uppercase`, `tracking-wider`, `text-sm`, `font-semibold`
- Code: `text-blue-400` (not the accent), monospace, `overflow-x-auto`

**Mobile**: Stacks vertically, single column

## 10. Navigation & Footer

### Sticky Navigation
- Fixed to top with `sticky`, `top-0`, `z-50`
- Semi-transparent dark backdrop: `rgba(25, 28, 31, 0.95)`
- Blur effect: `backdrop-blur-sm`
- Border below: `1px solid #2a2f37`
- Logo (left): `text-2xl font-bold tracking-tighter` ("audra")
- Actions (right): Primary + Secondary buttons, flex row gap

### Footer
- Border above: `1px solid #2a2f37`
- Two-row layout:
  1. Licensing section (max-width prose)
  2. Copyright + links (flex row, space-between)
- Links opacity: `opacity-60` default, `opacity-100` on hover
- Small text: `text-sm`, `opacity-60`

## 11. Do's and Don'ts

### Do
- Use `#191c1f` for all backgrounds — consistency is the design
- Apply `#4f55f1` to CTAs, numbers, and focal interactive elements only
- Keep button padding `1rem 2rem` — generous hit targets
- Use Inter weight 700 for all headings
- Border cards and sections with `#2a2f37` for visual hierarchy
- Maintain generous whitespace between sections (`py-24` / `py-32`)

### Don't
- Use shadows — Audra is flat by design
- Add gradients — the stark palette is intentional
- Overuse the accent color — it appears on ~3 interaction points per viewport
- Use serif fonts — this is a tech product, not editorial
- Round corners aggressively — `rounded-lg` (0.5rem) is the max
- Add decoration or ornament — every pixel should earn its place

## 12. Scrollbar Styling

Custom scrollbar (Chrome/Safari/Edge):
- Track: `#191c1f` (match background)
- Thumb: `#4f55f1` (match accent)
- Thumb radius: `4px` (slight curve)
- Width: `8px` (subtle)

## 13. Agent Prompt Guide

When iterating on Audra's landing page, use this shorthand:

**Quick Start**
> "Dark brutalist landing page. Background `#191c1f`, white text, single accent `#4f55f1`. Inter font, zero shadows, flat design. Borders `#2a2f37` for separation."

**Color Reference**
- Dark: `#191c1f`
- White: `#ffffff`
- Border: `#2a2f37`
- Accent: `#4f55f1`

**Component Shorthand**
- "Button: `#4f55f1` bg, white text, `1rem 2rem` padding, `0.5rem` radius, hover opacity 0.85"
- "Card: `#191c1f` bg, `1px #2a2f37` border, `1.5rem` padding, `0.5rem` radius, hover border lightens"
- "Heading: Inter weight 700, line-height 1.2–1.3, no tracking"

**Interaction Patterns**
- Buttons: Opacity fade on hover (0.85)
- Outlined buttons: Background tint on hover (`rgba(79, 85, 241, 0.1)`)
- Links: Opacity shift (0.6 → 1.0)
- Cards: Border lightens on hover (`#2a2f37` → `#4a5263`)

## 14. Accessibility Notes

- **Contrast**: Dark background + white text exceeds WCAG AAA (21:1 contrast)
- **Focus states**: All interactive elements have visible focus rings (browser default or custom)
- **Hit targets**: Buttons minimum 44px (48px with padding on most)
- **Semantic HTML**: Proper heading hierarchy (H1 → H2 → H3), nav landmark, footer landmark
- **Skip links**: Consider adding skip-to-content link for keyboard nav
- **Color independence**: Accent color used with text labels; never color-only

## 15. Performance Notes

- No external images, only semantic CSS
- Font: System stack + Inter (native or fallback)
- No animations (future: consider subtle fade-ins on scroll)
- No JavaScript required for critical path
- CSS-only hover states
- Minimal paint/reflow on interaction
