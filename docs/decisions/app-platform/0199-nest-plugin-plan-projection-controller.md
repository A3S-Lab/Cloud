# 0199. Nest-macro plugin plan projection controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Plugin plan projection confirmation (PUT) and get (GET) are thin
tenant-scoped handlers previously wrapped by builder helpers.

## Decision

Convert both factories to Nest macros with `OrganizationTenantGuard`,
scopes metadata (`PLUGIN_WRITE` / `CLOUD_READ`), and `raw` handlers. Keep
factories for module registration. Refuse inventing `S0` BYOK and refuse
marking Nest migrations as full production release.

## Consequences

Adds Nest precedent for thin plugin projection reads/writes. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
