# 0206. Nest-macro organization create controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Organization create is a thin platform-scoped POST without tenant
guard (bootstrap create).

## Decision

Convert `organization_controller` to Nest macros with `PLATFORM_WRITE` scopes
metadata and a `raw` POST handler. Keep the factory for module registration.
Refuse inventing `S0` BYOK and refuse marking Nest migrations as full production
release.

## Consequences

Adds Nest create-organization surface. Production release remains blocked on
honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
