# 0261. Nest-macro Identity bootstrap controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Identity bootstrap HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Identity bootstrap is a single public POST with an injected
`BootstrapGuard` (shared bootstrap credential), still on the builder
controller path.

## Decision

1. Convert bootstrap POST to Nest macros
   (`#[controller("/bootstrap")]` / `#[metadata("auth.public", true)]` /
   `#[post("/", raw)]`).
2. Keep `BootstrapGuard` injected at module wiring via
   `.controller()?.with_guard(guard)` so credential binding stays explicit.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for Identity bootstrap HTTP. Guard injection remains
wiring-owned. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_bootstrap_*` lib tests
