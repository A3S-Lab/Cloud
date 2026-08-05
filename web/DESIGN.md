---
name: A3S OS
description: A visible, high-trust control path for operating Agents and applications on owned infrastructure.
colors:
  primary: "#1264ff"
  primary-strong: "#0758ec"
  primary-soft: "#edf4ff"
  canvas: "#f7f9fd"
  panel: "#ffffff"
  panel-raised: "#f3f7ff"
  line: "#dce6f6"
  line-bright: "#c8d8f2"
  text: "#101828"
  muted: "#5d6b82"
  success: "#0c9b68"
  success-soft: "#e9f8f1"
  warning: "#b96e00"
  warning-soft: "#fff5df"
  danger: "#c93f50"
  danger-soft: "#fff0f2"
typography:
  display:
    fontFamily: "Segoe UI Variable, Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, system-ui, sans-serif"
    fontSize: "clamp(44px, 3.7vw, 60px)"
    fontWeight: 760
    lineHeight: 1.05
    letterSpacing: "-0.04em"
  headline:
    fontFamily: "Segoe UI Variable, Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, system-ui, sans-serif"
    fontSize: "clamp(29px, 3vw, 40px)"
    fontWeight: 760
    lineHeight: 1.1
    letterSpacing: "-0.035em"
  title:
    fontFamily: "Segoe UI Variable, Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 740
    lineHeight: 1.25
    letterSpacing: "-0.025em"
  body:
    fontFamily: "Segoe UI Variable, Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.55
    letterSpacing: "normal"
  label:
    fontFamily: "Segoe UI Variable, Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 650
    lineHeight: 1.4
    letterSpacing: "normal"
rounded:
  control: "8px"
  row: "10px"
  surface: "12px"
  shell: "18px"
  pill: "999px"
spacing:
  xxs: "4px"
  xs: "8px"
  sm: "12px"
  md: "16px"
  lg: "24px"
  xl: "32px"
  section: "56px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.panel}"
    rounded: "{rounded.control}"
    padding: "0 18px"
    height: "44px"
  button-secondary:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "36px"
  input:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "0 13px"
    height: "44px"
  card:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    rounded: "{rounded.surface}"
    padding: "22px"
  nav-tab:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    padding: "0"
    height: "57px"
  status-field:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.panel}"
    rounded: "{rounded.surface}"
    padding: "12px 18px"
    height: "76px"
  authority-row:
    backgroundColor: "{colors.canvas}"
    textColor: "{colors.text}"
    rounded: "{rounded.row}"
    padding: "8px 13px"
    height: "57px"
  state-badge:
    backgroundColor: "{colors.success-soft}"
    textColor: "{colors.success}"
    rounded: "{rounded.pill}"
    padding: "5px 9px"
  language-switcher:
    backgroundColor: "{colors.panel-raised}"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "3px"
    height: "34px"
---

# Design System: A3S OS

## Overview

**Creative North Star: "The Visible Control Path"**

A3S OS is a bright enterprise AI product portal with A3S Web as its operations workspace. A pure white canvas, cool blue hairlines, compact typography, and one electric-blue field explain product value and expose current state without turning the product into a generic infrastructure dashboard.

The public homepage leads with three outward-facing products: Workflow, Agent Factory, and the security operations center. It then reveals the shared A3S foundation, the complete gate-driven portfolio, and the authority that owns each outcome. Familiar controls, restrained motion, and semantic state color keep both the product story and authenticated tool trustworthy.

**Key Characteristics:**

- White and pale-blue operating surfaces with one dominant electric-blue field.
- A three-product application layer over one Cloud, Runtime, Box, Gateway, Code, and trust foundation.
- All 17 product gates shown with authoritative delivery states and no invented completion percentage.
- Compact, factual hierarchy designed for repeated use by platform operators.
- Explicit control and execution ownership, ending at the sole A3S Code Harness.
- Near-flat surfaces, cool hairlines, and sparse blue-tinted elevation.
- Structural responsive reflow with preserved labels and touch targets.
- Simplified Chinese by default with a complete, persistent English product version.

## Colors

The palette uses one action blue, cool neutral surfaces, and state colors reserved for operational meaning.

### Primary

- **Control Blue:** Primary actions, current navigation, focus, and the dominant convergence or architecture field.
- **Control Blue Strong:** Hover and pressed states for the primary action.
- **Control Blue Wash:** Selected rows, icon wells, and low-emphasis blue context.

### Neutral

- **Operator Canvas:** The cool page region behind authenticated workspaces.
- **Control Panel:** The white surface for cards, controls, and the sign-in canvas.
- **Raised Panel:** A pale-blue shell used around grouped authority or credential content.
- **Cool Hairline:** Standard card, divider, and control boundary.
- **Bright Hairline:** Focus-adjacent boundaries and stronger nested separators.
- **Ink:** Headings, primary labels, and authoritative values.
- **Slate:** Supporting copy and metadata.

### Semantic

- **Evidence Green:** Healthy, succeeded, ready, and verified states only.
- **Attention Amber:** Suspended, pending, retrying, and cleanup states.
- **Failure Red:** Failed, cancelled, unavailable, and destructive states.

**The One Blue Field Rule.** Give each viewport one dominant blue field; use all other blue as action, selection, or wayfinding.

**The Semantic Color Rule.** Green, amber, and red must communicate real state and never decorate neutral content.

## Typography

**Display Font:** Segoe UI Variable with Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, and system sans fallbacks

**Body Font:** Segoe UI Variable with Microsoft YaHei UI, PingFang SC, Noto Sans CJK SC, Aptos, Segoe UI, and system sans fallbacks

**Label/Mono Font:** The same sans for interface labels; the existing UI monospace stack is reserved for logs, commands, identifiers, and JSON.

**Character:** One compact humanist family carries the product. Weight and scale establish authority while sentence case and restrained tracking keep dense operational views easy to scan.

### Hierarchy

- **Display:** Used only for the sign-in promise, balanced to a short line measure.
- **Headline:** Workspace and primary page titles.
- **Title:** Card, module, and panel headings.
- **Body:** Explanations and operator guidance, normally held near 65 characters per line.
- **Label:** Context, metadata, counts, and control labels in sentence case.

**The Scan First Rule.** Establish hierarchy with size and weight; do not add visible eyebrow labels above headings.

**The Data Voice Rule.** Monospace belongs to code, commands, identifiers, and measured output, not to product personality.

## Language Versions

Simplified Chinese is the default product version on first visit. English is a complete alternate version, selected through the same compact segmented control on the public header and authenticated top bar. An explicit selection persists in local storage and updates the document `lang` attribute; it never changes authentication, tenancy, API behavior, or execution ownership.

Documentation adds an independent `main` / `v0.1.x` selector. `main` follows the current branch and roadmap snapshot. `v0.1.x` describes package `0.1.0` and REST contract `1.6.0`; it does not imply that every roadmap gate is verified. The selected documentation line persists separately from language.

Keep product and protocol names such as A3S OS, A3S Web, A3S Code Harness, A3S Flow, A3S Runtime, A3S Box, A3S Gateway, Agent, MCP, Skill, OCI, and SBOM unchanged. Translate surrounding operational language, status labels, relative time, timestamps, accessibility labels, loading states, empty states, and error summaries together.

**The Language Parity Rule.** A visible feature is complete only when its Chinese and English copy, state labels, dates, and accessible names describe the same product behavior.

## Layout

The authenticated shell uses a 64px top bar, a 58px navigation and tenant-context rail, and a workspace container with responsive horizontal padding. Overview pairs an operational column with a narrower authority column; supporting surfaces follow below.

The public homepage uses a 72px single-line desktop header, an asymmetric product promise and real control-plane access composition, a factual project strip, a three-cell asymmetric product layer, the complete gate portfolio, the live architecture diagram, and a versioned documentation workspace. Product value precedes implementation evidence: three products first, shared foundation second, roadmap gates third.

At 1180px, wide authority and status layouts compact. At 960px, major two-column compositions stack. At 780px, the operations drawer becomes an overlay and workspace modules become single-column. At 560px, navigation and tenant context scroll horizontally while cards, authority rows, and architecture modules use the full available width.

Spacing follows an 8px-centered rhythm, with 12px and 16px inside compact groups, 22px to 24px inside surfaces, and 32px or more between major regions.

**The Authority Beside Action Rule.** When width permits, place current operational truth next to the A3S component that owns the next transition.

## Elevation & Depth

The system is near-flat. Cool hairlines and pale-blue tonal layering establish structure; soft blue-tinted shadows are reserved for the sign-in shell, floating search results, dialogs, and the operations drawer.

### Shadow Vocabulary

- **Ambient Soft:** A low-contrast blue lift for contained grouped shells.
- **Ambient Float:** A larger blue lift for overlays and temporary surfaces that must separate from the workspace.
- **Drawer Separation:** A one-sided soft shadow that marks the operations drawer as an independent inspection surface.

**The Hairline Before Shadow Rule.** Use a cool boundary for structural surfaces and reserve visible shadow for shells or overlays that genuinely sit above them.

## Shapes

Controls use compact 8px corners, authority and list rows use 9px to 10px corners, and primary surfaces use 12px corners. The grouped sign-in shell may use 18px corners. Full pills are limited to small status, count, and connection controls.

The recurring silhouette is rectangular and aligned, with one rounded blue field rather than a page full of floating rounded containers.

## Components

### Buttons

- **Primary:** Solid Control Blue, white label, 44px target, and an 8px corner.
- **Secondary:** White or pale-neutral surface with a cool hairline and the same corner vocabulary.
- **Hover / Focus / Active:** Stronger blue on hover, a visible blue focus ring, and a one-pixel pressed translation.
- **Disabled / Loading:** Preserve the control footprint; reduce emphasis and keep the action label specific.

### Chips

- **Style:** Small, bordered pills with sentence-case text.
- **State:** Semantic fills are permitted only when the chip reports a real status. Count pills stay neutral or blue-soft.

### Cards / Containers

- **Corner Style:** Compact surface corners with cool 1px boundaries.
- **Background:** White by default; pale blue only for grouped shells and nested operational context.
- **Shadow Strategy:** Flat at rest; see the elevation rules for shells and overlays.
- **Internal Padding:** Usually 22px to 24px, reduced for dense rows and lists.

### Inputs / Fields

- **Style:** White field, cool border, explicit label, and 8px corner.
- **Focus:** Blue ring plus a strengthened boundary, without layout shift.
- **Error / Disabled:** Error copy names the problem; disabled controls remain legible and keep their full size.

### Navigation

Navigation is a single-line tab rail with Lucide line icons, compact labels, neutral inactive states, and a 3px blue underline for the active workspace. On narrow screens it scrolls instead of wrapping or shrinking labels into ambiguity.

### Language Switcher

Use one compact two-option fieldset labeled “Language”, with `中文` and `EN` buttons exposing `aria-pressed`. The active option sits on white with Control Blue text; the group uses a pale-blue field and an 8px corner. It must remain visible on the sign-in header and authenticated top bar.

### Convergence Field

The signature status field combines an icon and plain-language convergence statement with a compact set of factual counts. It is the dominant blue region and must use live environment projections.

### Authority Chain

Authority rows use a consistent icon well, label, and one-line detail. The chain ends in a solid-blue A3S Code Harness row to make the sole Agent execution owner unmistakable.

### Product Pillars

Workflow occupies the dominant electric-blue field because autonomous orchestration is the broadest product story. Agent Factory and the security operations center use smaller white and pale-blue fields. The three cells are intentionally asymmetric and each names its technical foundation, current delivery state, customer outcome, core capabilities, and contributing roadmap gates.

### Capability Portfolio

The portfolio is an evidence surface, not a marketing checklist. It shows the exact 17 gates in four product groups with Verified, In progress, Box re-certification, Planned, and explicit unavailable labels. A factual count field replaces completion bars or subjective percentages. Gate outcomes and key features remain visible without opening drawers.

### Architecture Map

The exported HTML map adds the three outward-facing products above unified access and control, then includes all 17 gates, Fleet convergence, Runtime and Box execution, Gateway and Power payload boundaries, and infrastructure trust. State styling and a text legend travel into the PNG export. The A3S Code Harness command remains unique and exact.

### Documentation Workspace

Documentation uses a stable index, one reading pane, an authoritative source link, and a separate version selector. The responsive layout turns the index into a horizontal rail before the reading pane stacks. Code samples use the one approved dark code surface; the page itself remains light.

## Do's and Don'ts

### Do:

- **Do** show the selected organization, project, and environment before operational detail.
- **Do** pair state with text or an icon instead of relying on color alone.
- **Do** keep standard controls familiar, keyboard reachable, and at least 40px to 44px on touch layouts.
- **Do** use real API projections and label planned architecture modules explicitly.
- **Do** lead the public story with Workflow, Agent Factory, and security operations before exposing gate evidence.
- **Do** keep the homepage, architecture map, and capability documentation on one shared 17-gate catalog.
- **Do** preserve the A3S authority path and the sole A3S Code Harness boundary.
- **Do** keep default Chinese and selectable English at feature parity, including dynamic states and accessible names.

### Don't:

- **Don't** create a second Harness, execution owner, or parallel orchestration story.
- **Don't** scatter full-saturation blue across inactive surfaces.
- **Don't** use visible eyebrow labels, gradient text, decorative glass, or technical monospace as costume.
- **Don't** invent customer names, scale metrics, availability claims, or completed roadmap capabilities.
- **Don't** reduce delivery progress to a made-up percentage or hide Box re-certification behind an Implemented label.
- **Don't** describe Workflow, Agent Factory, or the security operations center as separate control planes or runtimes.
- **Don't** compress desktop layouts onto mobile; reflow modules and allow purposeful horizontal scrolling for tab or tenant rails.
- **Don't** infer the default language from the browser or create per-page language state.
