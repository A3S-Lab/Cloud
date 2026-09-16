# 0273. Nest-macro Assets list query

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Assets read HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Asset list is org-scoped without deferred resource scope; asset and
release reads require deferred resource admission. Cloud read tenant admission
must stay on the cloud read entry helper.

## Decision

1. Convert asset list GET to Nest macros
   (`#[controller("/organizations")]` / `#[get(..., raw)]`).
2. Keep get/releases/selection attached via
   `.route(with_deferred_resource_scope(...))` after `.controller()`.
3. Keep `organization_tenant_cloud_read_controller` as the sole cloud read entry
   helper for tenant admission.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for asset list. Deferred reads and the cloud read entry
helper remain explicit. Production release remains blocked on honest remaining
gates.

## Evidence

- This ADR
- Focused `nest_macro_asset_queries_*` lib tests
