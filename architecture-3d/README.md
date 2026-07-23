# A3S Cloud Interactive Architecture

An independent Rsbuild, React, and Three.js application for exploring the A3S
Cloud control plane, managed node plane, provider resources, and delivery
roadmap. The scene is generated from typed architecture data and does not depend
on the Cloud API or management console.

Production site: <https://a3s-lab.github.io/Cloud/>

## Experience

- Explore five labeled domains from a high bird's-eye view, with a distinct
  facility and generated product badge for every module or middleware.
- Distinguish animated business flow, dashed structural/hosting relationships,
  and raised carrier chassis.
- See A3S Code as one workload hosted by a local A3S Box, while the separate
  A3S Box Runtime provider carries general Cloud OCI workload units.
- Inspect CPU and GPU hardware as multi-rack clusters without implying that the
  current Box provider already supports GPU passthrough.
- Run CPU deployment, source-to-OCI, GPU inference, live traffic, and
  logs/recovery scenarios from either A3S Web or A3S Code TUI.
- Orbit, zoom, select, focus, and inspect ownership, boundaries, placement,
  connected signals, roadmap status, and design references.
- Fall back to an accessible component index when WebGL is unavailable.
- Adapt the controls and details inspector to desktop and mobile viewports
  without resizing the Three.js canvas when a panel opens.
- Respect the operating system reduced-motion preference.

The typed data in [`src/architecture.ts`](src/architecture.ts),
[`src/topology.ts`](src/topology.ts), and
[`src/simulations.ts`](src/simulations.ts) is the source of truth for domains,
nodes, business edges, structural placement, scenarios, and roadmap status.
Keep it aligned with the Cloud domain model, technical architecture,
development plan, inference plan, and each provider's verified capability
boundary.

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
│   ├── architecture-schema.ts
│   ├── architecture.ts     # domains, nodes, business edges, and journeys
│   ├── topology.ts         # carrier and structural relationship model
│   ├── simulations.ts      # interactive business-flow scenarios
│   ├── app.tsx             # application shell and exploration state
│   └── main.tsx            # Rsbuild browser entry point
├── rsbuild.config.ts
└── package.json
```

The Three.js runtime is deliberately separate from React lifecycle and
presentation state, following the same runtime/component boundary used by
`apps/windhole`.
