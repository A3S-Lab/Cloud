# 0182. Nest-macro organizations list query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

ADR `0179` established Nest-style attribute macros as the preferred Boot
controller surface and converted the thin public `platform` module. Cloud still
registers the majority of controllers through fluent builders.

`organizations_query_controller` is a thin GET-only Identity presentation
surface (`GET /organizations`) that already fits Nest macros with a `raw`
handler returning `BootResponse`, without inventing behavior.

## Decision

1. **Convert** `organizations_query_controller` to `#[controller]` / `#[get("/", raw)]`
   / `#[metadata("auth.scopes", ...)]`, keeping the factory
   `organizations_query_controller(bus)` for module registration compatibility.
2. **Prefer** Nest macros for similarly thin query controllers when touched.
3. **Refuse** mass-rewriting complex command controllers with dynamic status
   codes in this slice.

## Consequences

- Adds a second Nest-macro precedent on an authenticated, scoped management
  route.
- Does not declare production release complete.

## Evidence

- This ADR; `organizations_query_controller.rs`
- Tests: `cargo test -p a3s-cloud-control-plane --lib nest_macro_organizations_query_controller`
