# 0201. Nest-macro API token controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. API token list/get/create/revoke are thin tenant-scoped handlers with
a single `TOKEN_WRITE` scope.

## Decision

Convert `api_token_controller` to Nest macros with `OrganizationTenantGuard`,
`TOKEN_WRITE` scopes metadata, and `raw` GET/POST/DELETE handlers. Keep the
dual-bus factory for module registration. Refuse inventing `S0` BYOK and refuse
marking Nest migrations as full production release.

## Consequences

Adds Nest dual-bus command/query controller precedent. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
