# A3S Cloud Interactive Architecture

An independent Rsbuild, React, and Three.js application for exploring the A3S
Cloud control plane, managed node plane, provider resources, and delivery
roadmap. The scene is generated from typed architecture data and does not depend
on the Cloud API or management console.

Production site: <https://a3s-lab.github.io/Cloud/>

## Experience

- Orbit, zoom, select, and focus individual architecture components.
- Trace deploy, source/build, traffic, and observation journeys.
- Inspect ownership, boundary rules, connected signals, roadmap status, and
  design references.
- Fall back to an accessible component index when WebGL is unavailable.
- Adapt the controls and details inspector to desktop and mobile viewports
  without resizing the Three.js canvas when a panel opens.
- Respect the operating system reduced-motion preference.

The graph in [`src/architecture.ts`](src/architecture.ts) is the source of truth
for layers, nodes, edges, journeys, and roadmap status. Keep it aligned with the
Cloud domain model, technical architecture, development plan, and inference
plan.

## Local development

Requirements:

- Bun 1.3.14
- Node.js 22 or later

```bash
bun install --frozen-lockfile
bun run dev
```

Rsbuild serves the application at <http://127.0.0.1:4173/> by default. Override
the port with `A3S_ARCHITECTURE_DEV_PORT`.

Run the complete local verification suite before submitting a change:

```bash
bun run typecheck
bun run format:check
bun run lint:check
bun run test
bun run build
```

## GitHub Pages

The
[`architecture-3d-pages.yml`](../.github/workflows/architecture-3d-pages.yml)
workflow validates every pull request that changes this application. A change
merged to `main` builds with the repository path prefix, uploads `dist/` as the
official Pages artifact, and deploys it to the `github-pages` environment.

For a local production build that uses the same URL layout:

```bash
A3S_ARCHITECTURE_BASE_PATH=/Cloud/ bun run build
```

The Cloud repository must use **GitHub Actions** as its Pages source. The
workflow needs no branch containing generated assets and does not commit
`dist/`.

## Structure

```text
architecture-3d/
├── public/                 # favicon and GitHub Pages .nojekyll marker
├── src/
│   ├── components/         # React scene bridge and details inspector
│   ├── scene/              # Three.js runtime, visuals, and interaction
│   ├── styles/             # responsive application presentation
│   ├── architecture.ts     # typed architecture graph
│   ├── app.tsx             # application shell and exploration state
│   └── main.tsx            # Rsbuild browser entry point
├── rsbuild.config.ts
└── package.json
```

The Three.js runtime is deliberately separate from React lifecycle and
presentation state, following the same runtime/component boundary used by
`apps/windhole`.
