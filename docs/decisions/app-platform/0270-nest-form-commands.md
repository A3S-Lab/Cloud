# 0270. Nest-macro Forms create command

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Forms write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Form draft create is project-scoped without deferred project scope;
revise and publish require deferred project admission. Forms tenant admission
must stay on the Forms entry helper.

## Decision

1. Convert form create POST to Nest macros
   (`#[controller("/organizations")]` / `#[post(..., raw)]`).
2. Keep revise and publish attached via
   `.route(with_deferred_project_scope(...))` after `.controller()`.
3. Keep `organization_tenant_form_write_controller` as the sole Forms write
   entry helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for form create. Deferred revise/publish and the Forms
entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_form_commands_*` lib tests
