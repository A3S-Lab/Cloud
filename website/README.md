# A3S OS Website and Documentation

The public site contains two independent Rspress builds in one package:

- the product application at `/Cloud/`, which explains the desired-state
  A3S OS product system through three product chapters, the A3S Work edge Agent,
  the shared infrastructure map, and honest delivery gates;
- the versioned documentation application at `/Cloud/docs/`, where Simplified
  Chinese is the default language and English is available from the native
  language switcher.

Neither application depends on the Cloud API or operations console.

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
npm run dev:docs
```

`npm run dev` starts the product site. `npm run dev:docs` starts the
documentation application.

Roadmap gate metadata is generated from `../ROADMAP.md`; the website does not
maintain an independent status model. Regenerate and validate it with:

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

`npm run build` builds both Rspress applications and assembles documentation
under `doc_build/docs/`. `npm run lint` also verifies the documentation version
registry, the workspace-version alignment, and exact Chinese/English page
parity.

## Documentation versions and languages

The single documentation locale and version registry is
`documentation/versions.json`. Content is stored under
`documentation/<version>/<language>/`; do not create a handwritten version or
language selector.

| Version and language               | Public route           |
| ---------------------------------- | ---------------------- |
| Default `v0.1`, Simplified Chinese | `/Cloud/docs/`         |
| Default `v0.1`, English            | `/Cloud/docs/en/`      |
| `next`, Simplified Chinese         | `/Cloud/docs/next/`    |
| `next`, English                    | `/Cloud/docs/next/en/` |

The default version must match the major and minor series in the workspace
`Cargo.toml`. The `next` channel describes behavior under development and is
not release evidence. To publish a new minor documentation version:

1. freeze the matching Chinese and English `next` pages into a new version
   directory;
2. retain the previous version directory as an immutable snapshot;
3. update `documentation/versions.json` once;
4. run the complete verification suite.

Every version must expose the same page paths in Chinese and English. The build
fails when a translation, registered directory, per-version search index, or
public route is missing. Locale auto-redirect is disabled so the unprefixed
documentation route always remains Simplified Chinese.

The Canvas UI attribution and applicable license terms live in
`THIRD_PARTY_NOTICES.md`.

## Production layout

Set `DOCS_BASE=/Cloud/` when building for GitHub Pages. The product build uses
that base, while the documentation build derives `/Cloud/docs/` from it. The
repository's single Pages workflow then builds the architecture app with
`A3S_ARCHITECTURE_BASE_PATH=/Cloud/architecture/`, copies it into
`doc_build/architecture/`, verifies all public applications and routes, and
uploads one Pages artifact.
