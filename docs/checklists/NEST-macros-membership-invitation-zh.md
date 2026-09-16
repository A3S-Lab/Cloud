# Nest macros membership invitation controllers

## Scope

- Membership invitation administration/self-list/acceptance HTTP adapters
  migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / dual `#[use_guard]` where required / `#[get|post(..., raw)]`
- [x] scopes metadata (`IDENTITY_WRITE` / `CLOUD_READ`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0221`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
