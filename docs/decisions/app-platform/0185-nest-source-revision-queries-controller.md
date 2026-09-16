# 0185. Nest-macro source revision query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin query
surfaces. `source_revision_queries_controller` is a guarded GET list with no
command mutations and fits `#[use_guard]` / `#[get(..., raw)]`.

## Decision

Convert `source_revision_queries_controller` to Nest macros while keeping the
factory registration. Refuse inventing deferred product surfaces or `S0` BYOK.

## Consequences

Adds Sources Nest query precedent. Does not complete production release.

## Evidence

- This ADR; `source_revision_queries_controller.rs`
- Tests: `cargo test -p a3s-cloud-control-plane --lib nest_macro_source_revision_queries_controller`
