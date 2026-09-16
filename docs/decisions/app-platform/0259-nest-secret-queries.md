# 0259. Nest-macro Secrets query list controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Secrets HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Secrets list/get still used the builder controller with
`organization_tenant_secret_read_controller` (tenant guard only). Org-scoped
get-by-id still requires deferred project-scope admission after load.

## Decision

1. Convert Secrets list to Nest macros (`#[controller]` / `#[get(..., raw)]`).
2. Keep tenant admission on `organization_tenant_secret_read_controller`
   (architecture boundary; no direct `OrganizationTenantGuard` in the module).
3. Keep org-scoped `GET .../secrets/{secret_id}` on
   `.route(with_deferred_project_scope(...))` so deferred visibility admission
   is unchanged.
4. Refuse inventing `S0` BYOK / remaining planned foreign gates, and refuse
   treating Nest migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for Secrets list HTTP. Deferred get remains an explicit
non-macro route. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_secret_queries_*` lib tests
