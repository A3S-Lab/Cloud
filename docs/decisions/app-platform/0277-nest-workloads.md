# 0277. Nest-macro Workloads create commands

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Workloads write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Workload create, source-create, and agent-create are
environment-scoped without deferred project scope. Update/rollback/bind/stop/
cancel require deferred project admission. Tenant admission must stay on the
Workloads entry helper.

## Decision

1. Convert the three create POSTs to Nest macros
   (`#[controller("/organizations")]` / `#[post(..., raw)]`).
2. Keep update/rollback/bind/unbind/stop/cancel attached via
   `.route(with_deferred_project_scope(...))` after `.controller()`.
3. Keep `organization_tenant_workload_write_controller` as the sole Workloads
   write entry helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for three workload create surfaces. Deferred mutations and
the Workloads entry helper remain explicit. Production release remains blocked
on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_workloads_*` lib tests
