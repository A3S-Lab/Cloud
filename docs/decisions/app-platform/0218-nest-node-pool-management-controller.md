# 0218. Nest-macro fleet node-pool management commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Node-pool create/member/maintenance mutations are thin tenant-scoped
fleet commands without deferred resource scope. Sibling node-pool query
controller already uses Nest macros (ADR `0192`).

## Decision

Convert `node_pool_management_controller` to Nest macros with
`OrganizationTenantGuard`, `NODE_WRITE` scopes metadata, and `raw` POST
handlers. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full public production release.

## Consequences

Adds Nest coverage for fleet node-pool management commands. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
