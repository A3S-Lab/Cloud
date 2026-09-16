# 0213. Nest-macro domain claim commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Domain claim create/verify/revoke are thin tenant-scoped edge commands
without deferred resource scope and support the APP0.6 custom-domain claim path.

## Decision

Convert `domain_claim_commands_controller` to Nest macros with
`OrganizationTenantGuard`, `ROUTE_WRITE` scopes metadata, and `raw` POST
handlers. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full public production release.

## Consequences

Adds Nest coverage for domain claim commands. `APP0.6` remains `implemented`
with `parity_claim=false`; public release stays blocked on remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
