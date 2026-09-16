# 0272. Nest-macro Assets create command

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Assets write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Asset create is org-scoped without deferred resource scope;
archive/release/yank require deferred resource admission. Assets tenant
admission must stay on the Assets entry helper.

## Decision

1. Convert asset create POST to Nest macros
   (`#[controller("/organizations")]` / `#[post(..., raw)]`).
2. Keep archive/release/yank attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Keep `organization_tenant_asset_write_controller` as the sole Assets write
   entry helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for asset create. Deferred mutations and the Assets entry
helper remain explicit. Production release remains blocked on honest remaining
gates.

## Evidence

- This ADR
- Focused `nest_macro_asset_commands_*` lib tests
