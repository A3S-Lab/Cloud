# Nest macros recipient contact

## Scope

- Recipient contact query and command HTTP adapters migrated to Nest macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[get|post(..., raw)]`
- [x] scopes metadata (`CLOUD_READ` / `IDENTITY_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0208`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations as full public production release.
