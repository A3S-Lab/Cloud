# 0280. Nest-macro User Files queries

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / User Files read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. User Files list/get/content are project-scoped reads without deferred
resource scope. Organization quota requires deferred resource admission. Tenant
admission must stay on the cloud read entry helper.

## Decision

1. Convert list/get/content GETs to Nest macros
   (`#[controller("/organizations")]` / `#[get(..., raw)]`).
2. Keep quota attached via `.route(with_deferred_resource_scope(...))` after
   `.controller()`.
3. Keep `organization_tenant_cloud_read_controller` as the sole User Files read
   entry helper for tenant admission.
4. Leave User Files command surfaces on imperative `ControllerDefinition` for a
   follow-on Nest hybrid (content PUT needs octet-stream body limits).
5. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for User Files reads. Deferred quota and the cloud read
entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_user_file_queries_*` lib tests
