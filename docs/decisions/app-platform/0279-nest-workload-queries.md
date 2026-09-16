# 0279. Nest-macro Workloads list query

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Workloads read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Workload list is environment-scoped without deferred project scope.
Get/logs/SSE require deferred project admission. Tenant admission must stay on
the Workloads read entry helper.

## Decision

1. Convert list GET to Nest macros
   (`#[controller("/organizations")]` / `#[get(..., raw)]`).
2. Keep get/logs/SSE attached via `.route(with_deferred_project_scope(...))`
   after `.controller()`.
3. Keep `organization_tenant_workload_read_controller` as the sole Workloads
   read entry helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for workload list. Deferred reads/SSE and the Workloads
read entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_workload_queries_*` lib tests
