# 0202. Nest-macro plugin assignment controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Plugin assignment list/get and set are thin tenant-scoped handlers.

## Decision

Convert command and query factories to Nest macros with
`OrganizationTenantGuard`, scopes metadata (`PLUGIN_WRITE` / `CLOUD_READ`), and
`raw` handlers. Keep factories for module registration. Refuse inventing `S0`
BYOK and refuse marking Nest migrations as full production release.

## Consequences

Adds Nest plugin assignment surface. Production release remains blocked on
honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
