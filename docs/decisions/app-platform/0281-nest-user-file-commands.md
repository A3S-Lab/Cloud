# 0281. Nest-macro User Files commands

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / User Files write HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. User Files reserve/scan/expire/tombstone are project-scoped writes
without deferred resource scope. Content PUT requires octet-stream content-type
checks and body-size limits that stay on an imperative route. Tenant admission
must stay on the file write entry helper.

## Decision

1. Convert reserve/scan/expire/tombstone POSTs to Nest macros
   (`#[controller("/organizations")]` / `#[post(..., raw)]`).
2. Keep content PUT attached via `.route(RouteDefinition::put(...))` after
   `.controller()`.
3. Keep `organization_tenant_file_write_controller` as the sole User Files write
   entry helper for tenant admission and `FILE_WRITE` scope.
4. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for four User Files write surfaces. Content PUT and the file
write entry helper remain explicit. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_user_file_commands_*` lib tests
