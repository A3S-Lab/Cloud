# Nest macros domain claim + MCP credential commands

## Scope

- Domain claim create/verify/revoke and MCP credential create/rotate/revoke HTTP
  adapters migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[post(..., raw)]`
- [x] scopes metadata (`ROUTE_WRITE` / `MCP_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0213` / `0214`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
