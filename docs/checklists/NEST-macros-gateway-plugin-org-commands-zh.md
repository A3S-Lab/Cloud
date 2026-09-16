# Nest macros gateway-scope commands + plugin-registry commands + organization create

## Scope

- Gateway scope create, plugin registry enroll, and organization create HTTP
  adapters migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` (where applicable) / `#[post(..., raw)]`
- [x] scopes metadata (`ROUTE_WRITE` / `PLUGIN_WRITE` / `PLATFORM_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0204` / `0205` / `0206`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations as full public production release.
