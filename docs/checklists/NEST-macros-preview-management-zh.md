# Nest macros preview management controllers

## Scope

- Pull-request preview policy/preview HTTP adapters migrated to Nest attribute
  macros with root Presentation tenant-scope helpers.

## Done

- [x] Nest `#[controller]` / `#[get|post(..., raw)]`
- [x] helper wraps for build-write / cloud-read scopes
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0226`

## Explicit refusals

- Do not invent HA/ops console productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
