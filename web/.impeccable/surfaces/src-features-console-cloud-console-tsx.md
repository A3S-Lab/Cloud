---
version: 1
slug: "src-features-console-cloud-console-tsx"
primary_target: "src/features/console/cloud-console.tsx"
related_targets: ["src/features/session/sign-in.tsx","src/features/architecture/architecture-diagram.tsx"]
---

# A3S Cloud Web Redesign

## Scope and mode

- Scope: the complete authenticated console plus the token sign-in surface.
- Primary target: `src/features/console/cloud-console.tsx`.
- Related targets: Sign In, Overview, Workloads, Agents, Delivery, Edge, Architecture, search, logs, forms, and the operations drawer.
- Visitor mode: Operate. Expression must never obscure state, ownership, actions, or familiar controls.

## Audience, job, action, proof, and constraints

- Platform operators need to choose tenant context, understand convergence, perform lifecycle actions, and inspect authoritative evidence quickly.
- Agent engineers need to run and observe the existing A3S Code Harness without a second Harness or execution mechanism.
- Primary task: identify current state and move directly to the owning workspace or operation.
- Proof comes from real API projections, immutable revisions, operation state, build evidence, routes, certificates, logs, and semantic Agent events.
- Preserve all API contracts, deep links, streaming behavior, keyboard semantics, error handling, and architecture PNG export.
- Do not invent customers, metrics, pricing, completed roadmap features, or availability claims.

## Approved direction

- Approved comp: `.impeccable/mocks/overview-b-operations-first.png`.
- Application shell: comp B, an operations-first white enterprise console with one electric-blue status field and a compact authority rail.
- Architecture workspace: comp C's stacked control and execution bands, translated to the existing complete architecture content.
- Sign In: comp A's asymmetric story and system-map composition, with the credential form as the primary action.
- Benchmark: Finogeeks enterprise visual discipline, translated into original A3S product language rather than copied identity or content.
- Memorable moment: the blue convergence field and adjacent authority chain make current health and the sole execution path legible in one scan.

## Design-system inventory

| Ingredient | Commitment | Implementation medium |
| --- | --- | --- |
| App canvas | Pure white with very pale blue-gray work regions | Semantic HTML and CSS tokens |
| Brand field | Electric A3S blue, used for one dominant status or architecture band per viewport | CSS background |
| Typography | Compact humanist system sans, near-black headings, blue-gray body | CSS system font stack |
| Navigation | One-line desktop top bar and one-line workspace tab rail | Existing React semantics plus CSS |
| Tenant context | Compact labeled selects aligned with the navigation hierarchy | Existing form controls plus CSS |
| Overview | Environment heading, factual status band, asymmetric operational surfaces | Existing API data and React components |
| Authority chain | Operations + Flow, Workloads, Node Agent, Runtime + Box, Gateway, sole A3S Code Harness | Semantic HTML, icon library, and CSS connectors |
| Cards | 12px radius, 1px cool-blue hairline, near-flat elevation | CSS |
| Controls | 8px radius, clear labels, visible focus, 40-44px touch targets | Existing React controls plus CSS |
| Status | Text and icon first; green, amber, and red only for semantic state | Existing state data plus CSS |
| Architecture | Full responsive module map with export retained | Existing semantic HTML and export code, recomposed with CSS |
| Sign In | Left product story and truthful authority path, right credential card | React and CSS, no generated image shipped |
| Motion | Short opacity/translate entrances and control feedback only | CSS, disabled under reduced motion |

## Type, shape, line, and elevation grammar

- Display: 48-56px desktop, 34-40px compact, weight 700-760, tight tracking.
- Page title: 32-40px, weight 700.
- Section title: 18-22px, weight 700.
- Body: 14-16px, line-height 1.55-1.7.
- Metadata: 11-13px, sentence case. Avoid repeated uppercase eyebrow labels.
- Buttons and inputs: 8px radius. Surfaces: 12px radius. Status badges may use full radius.
- Borders: 1px cool-blue hairline. Shadows are sparse and blue-tinted, never black-heavy.
- One accent: `#1264ff`. Healthy mint is semantic only.

## Responsive behavior

- At 1024px and below, the top navigation becomes horizontally scrollable without wrapping, and context moves to its own row.
- At 780px and below, multi-column workspaces collapse to one column and the operations drawer becomes an overlay.
- At 560px and below, labels remain visible where needed, controls retain 44px targets, and architecture bands become scrollable or single-column without shrinking text below readability.

## Deliberate translations from the comp

- Generated counts and organization names in the comp are illustrative only and must never be shipped as product claims.
- Comp tables map onto the existing Workloads, Operations, Delivery, and Edge data instead of introducing mock records.
- The approved comp is a layout and design-system authority, not a source for raster UI or copied text.

## Unresolved decisions

- None blocking. The user delegated the detailed translation by confirming the recommended B + C + A composition.
