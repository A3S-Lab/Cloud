# 0205. Nest-macro plugin registry commands controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Plugin registry enroll is the command sibling of Nest query ADR `0200`.

## Decision

Convert `plugin_registry_commands_controller` to Nest macros with
`OrganizationTenantGuard`, `PLUGIN_WRITE` scopes metadata, and a `raw` POST
handler (replacing `organization_tenant_plugin_write_controller` helper wiring).
Keep the factory for module registration. Refuse inventing `S0` BYOK and refuse
marking Nest migrations as full production release.

## Consequences

Completes Nest coverage for plugin-registry enroll + queries. Production release
remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
