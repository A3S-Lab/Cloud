# 0271. Nest-macro Forms list query

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Forms read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Form draft list is project-scoped without deferred project scope;
org-scoped draft and release reads require deferred project admission. Forms
tenant admission must stay on the Forms entry helper.

## Decision

1. Convert form draft list GET to Nest macros
   (`#[controller("/organizations")]` / `#[get(..., raw)]`).
2. Keep draft/release gets attached via
   `.route(with_deferred_project_scope(...))` after `.controller()`.
3. Keep `organization_tenant_form_read_controller` as the sole Forms read entry
   helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for form list. Deferred org-scoped reads and the Forms
entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_form_queries_*` lib tests
