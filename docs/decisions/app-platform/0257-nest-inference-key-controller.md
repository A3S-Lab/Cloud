# 0257. Nest-macro inference key controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Inference key create/rotate/revoke/list are Identity-owned sibling
surfaces to Nest inference route controllers (ADRs `0194` / `0207`). Org-scoped
credential get still requires `DeferredResourceScope::Any` admission after load.

## Decision

1. Convert inference key commands and list query to Nest macros
   (`#[controller]` / `#[use_guard]` / `#[metadata]` / `#[post|get(..., raw)]`).
2. Keep the org-scoped get route on the builder `.route(with_deferred_resource_scope(...))`
   path so deferred visibility admission is unchanged.
3. Refuse inventing `S0` BYOK, `I0.6` provider productization, LLM node
   availability, and refuse marking Nest migrations as public release completion.

## Consequences

Adds Nest coverage for inference key lifecycle HTTP. Deferred get remains an
explicit non-macro route. Production release remains blocked on honest remaining
gates (`I0.6`/`S0`/`K0.*`/`U0.4`/`MCP0.5`).

## Evidence

- This ADR
- Focused `nest_macro_inference_key_*` lib tests
