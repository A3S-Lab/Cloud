# A3S Cloud Website

The A3S Cloud product website is an independent Rspress application. It
explains the desired-state control loop, current capabilities, product
boundaries, and planned delivery gates without depending on the Cloud API or
operations console.

The interactive architecture remains a separate Rsbuild application under
`../architecture-3d/`. The Pages workflow builds both applications and places
the architecture artifact at `/Cloud/architecture/` beneath this site's output.

## Local development

Requirements:

- Node.js 22 or later
- npm 10 or later

```bash
npm ci
npm run dev
```

The roadmap constellation is generated from `../ROADMAP.md`; it is not an
independent status model. Regenerate and validate it with:

```bash
npm run generate:roadmap
npm run check:roadmap
```

Run the product-site verification suite before submitting a change:

```bash
npm run format:check
npm run lint
npm run build
npm run check:site
```

The Canvas UI attribution and applicable license terms live in
`THIRD_PARTY_NOTICES.md`.

## Production layout

Set `DOCS_BASE=/Cloud/` when building for GitHub Pages. The repository's single
Pages workflow then builds the architecture app with
`A3S_ARCHITECTURE_BASE_PATH=/Cloud/architecture/`, copies it into
`doc_build/architecture/`, verifies both applications, and uploads one Pages
artifact.
