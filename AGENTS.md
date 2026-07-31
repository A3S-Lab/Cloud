# A3S Workflow contributor guide

A3S Workflow is an AI-native workflow engine. The Rust control plane is built
with A3S Boot and A3S Flow, PostgreSQL is the only authoritative store, and
every workflow node executes through the A3S Runtime contract. The Studio is a
Bun + Rsbuild + React application; Next.js and PNPM are not part of this repo.

## Invariants

- Never execute a workflow node inside the API or Flow worker process.
- Include start, approval, router, and output nodes in the Runtime boundary.
- Persist definitions, events, queue state, hooks, memory, and Runtime evidence
  in PostgreSQL. Do not introduce Redis as a source of truth.
- Keep Runtime artifacts content-addressed and verify input/output digests.
- Keep the bundled process Runtime provider development-only. Production
  providers must reject policies they cannot enforce.
- Preserve provider and pool placement so stateless nodes can scale
  independently from the control plane.

## Verification

Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --
-D warnings`, and `cargo test --workspace --all-targets`. In `web/`, run
`bun install --frozen-lockfile`, `bun run check`, `bun test`, and
`bun run build`. Browser changes must keep `tests/e2e/workflow-studio.acl`
green with the official `a3s-test` CLI.
