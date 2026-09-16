# Nest macros membership + resource grant controllers

## Scope

- Membership list/get/create/role/revocation HTTP adapters migrated to Nest
  attribute macros.
- Resource-grant list/get/create/revocation HTTP adapters migrated to Nest
  attribute macros.

## Done

- [x] Nest `#[controller]` / dual `#[use_guard]` / `#[get|post(..., raw)]`
- [x] scopes metadata (`IDENTITY_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0219` / `0220`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
