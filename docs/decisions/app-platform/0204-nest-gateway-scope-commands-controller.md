# 0204. Nest-macro gateway scope commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Gateway scope create is the command sibling of Nest query ADR `0184`.

## Decision

Convert `gateway_scope_commands_controller` to Nest macros with
`OrganizationTenantGuard`, `ROUTE_WRITE` scopes metadata, and a `raw` POST
handler. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full production release.

## Consequences

Completes Nest coverage for the gateway-scope HTTP pair. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
