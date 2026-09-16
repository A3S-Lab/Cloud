# Nest macros routes + MCP route policy + source revisions + enrollment

## Scope

- Route publish, MCP route policy create/revise, source revision resolve, and
  node enrollment HTTP adapters migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` (where applicable) / `#[post(..., raw)]`
- [x] scopes or public metadata (`ROUTE_WRITE` / `MCP_WRITE` / `SOURCE_WRITE` / `auth.public`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0209` / `0210` / `0211` / `0212`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations as full public production release.
