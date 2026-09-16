# 0267. Nest-macro Secrets create command

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Secrets write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Secret create is environment-scoped without deferred project scope;
rotate and revoke require deferred project admission. Secrets tenant admission
must stay on the Secrets entry helper (architecture boundary).

## Decision

1. Convert secret create POST to Nest macros
   (`#[controller("/organizations")]` / `#[post(..., raw)]`).
2. Keep rotate and revoke attached via
   `.route(with_deferred_project_scope(...))` after `.controller()`.
3. Keep `organization_tenant_secret_write_controller` as the sole Secrets
   write entry helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for secret create. Deferred rotate/revoke and the Secrets
entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_secrets_*` lib tests
