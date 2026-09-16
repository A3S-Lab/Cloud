# 0282. Nest-macro Build Run list query

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Build Run read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Build Run list is environment-scoped without deferred project scope.
Get/evidence/logs/SSE require deferred project admission. Tenant admission must
stay on the cloud read entry helper.

## Decision

1. Convert list GET to Nest macros
   (`#[controller("/organizations")]` / `#[get(..., raw)]`).
2. Keep get/evidence/logs/SSE attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Keep `organization_tenant_cloud_read_controller` as the sole Build Run read
   entry helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for Build Run list. Deferred reads/SSE and the cloud read
entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_build_run_queries_*` lib tests
