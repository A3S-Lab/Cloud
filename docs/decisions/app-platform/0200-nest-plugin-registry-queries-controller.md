# 0200. Nest-macro plugin registry queries controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Plugin registry list/get plus catalog search/inspect POSTs are thin
tenant-scoped JSON handlers.

## Decision

Convert `plugin_registry_queries_controller` to Nest macros with
`OrganizationTenantGuard`, `CLOUD_READ` scopes metadata, and `raw` GET/POST
handlers. Keep the factory for module registration. Refuse inventing `S0` BYOK
and refuse marking Nest migrations as full production release.

## Consequences

Adds Nest GET+POST query-surface precedent for plugin registries. Production
release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
