# A3S Cloud Interactive Architecture

An independent Rsbuild and React application with a Three.js 3D scene and an
embedded Archify 2D system map. Both views explain the A3S Cloud control plane,
middleware, managed node plane, providers, infrastructure, and delivery
roadmap without depending on the Cloud API or management console.

Production site: <https://a3s-lab.github.io/Cloud/architecture/>

## Experience

- Switch between tabs labeled `3D` and `2D`. The 2D tab embeds a validated,
  self-contained [Archify](https://github.com/tt-a1i/archify) artifact.
- Explore seven explicit layers from a high bird's-eye view: experience,
  public Gateway access, Cloud control domains, platform middleware/state,
  managed node Runtime, providers/workloads, and physical infrastructure.
- See A3S Gateway spatially between A3S Web / A3S Code and the private A3S Boot
  API instead of treating browser clients as direct control-plane callers.
- Distinguish animated business flow, dashed structural/hosting relationships,
  and raised carrier chassis.
- Select every component, business-flow line, or structural/hosting line to
  open a floating HUD with its purpose, endpoints, transferred facts,
  simulations, directional semantics, and boundary rule.
- See A3S Code as one workload hosted by a local A3S Box, while the separate
  Cloud-facing A3S Box provider is the sole Runtime execution and source-build
  path for general Cloud workload units.
- Distinguish the planned Cloud Inference bounded context from A3S Power, one
  optional TEE inference backend carried by an ordinary Box-hosted workload.
- Read `F0` as verified, `BX0` as in progress, and the prior `R0`, `N0`, `D0`,
  and `E0` evidence as historical until the Box-only baseline is re-certified.
- Inspect CPU and GPU hardware as multi-rack clusters without implying that the
  current Box provider already supports GPU passthrough.
- Run CPU deployment, source-to-OCI, A3S Power GPU inference, live traffic, and
  logs/recovery scenarios from either A3S Web or A3S Code TUI.
- Orbit, zoom, select, focus, and inspect ownership, boundaries, placement,
  connected signals, roadmap status, and design references.
- Fall back to an accessible component and relationship index when WebGL is
  unavailable.
- Adapt the controls and details inspector to desktop and mobile viewports
  without resizing the Three.js canvas when a panel opens.
- Respect the operating system reduced-motion preference.

The typed data in [`src/architecture.ts`](src/architecture.ts),
[`src/relationship-content.ts`](src/relationship-content.ts),
[`src/topology.ts`](src/topology.ts), and
[`src/simulations.ts`](src/simulations.ts) is the source of truth for domains,
nodes, detailed business edges, structural placement, scenarios, and roadmap
status. [`scripts/generate-archify-source.ts`](scripts/generate-archify-source.ts)
projects that same graph into Archify JSON.
Keep it aligned with the Cloud domain model, technical architecture,
development plan, inference plan, and the sole provider's current capability
boundary and roadmap state.

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

After changing architecture data, regenerate the checked Archify source:

```bash
bun run archify:source
```

Rendering instructions and the third-party MIT notice live under
[`archify/`](archify/). The validated standalone HTML is checked in under
`public/archify/` so local development and GitHub Pages never require an
external runtime or network request.

Run the complete local verification suite before submitting a change:

```bash
bun run typecheck
bun run format:check
bun run lint:check
bun run test
bun run build
```

## GitHub Pages

The [`cloud-pages.yml`](../.github/workflows/cloud-pages.yml) workflow validates
every pull request that changes this application. A change merged to `main`
builds the product website and architecture app, places this app under the
`architecture/` route, uploads one combined Pages artifact, and deploys it to
the `github-pages` environment.

For a local production build that uses the same URL layout:

```bash
A3S_ARCHITECTURE_BASE_PATH=/Cloud/architecture/ bun run build
```

The Cloud repository must use **GitHub Actions** as its Pages source. The
workflow needs no branch containing generated assets and does not commit
`dist/`.

## Structure

```text
architecture-3d/
├── archify/                # generated JSON source, regeneration notes, license
├── public/
│   └── archify/            # validated self-contained 2D HTML artifact
├── scripts/
│   └── generate-archify-source.ts
├── src/
│   ├── components/         # 3D/2D bridges and shared HUD inspectors
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

The Three.js runtime and Archify iframe bridge remain separate from React
lifecycle and presentation state. Both report selections into the same module
and relationship HUD state without resizing either visualization.
