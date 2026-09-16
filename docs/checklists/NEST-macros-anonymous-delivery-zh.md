# Nest macros anonymous application delivery controller

## Scope

- Anonymous delivery admission HTTP adapter migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[metadata("auth.public", true)]` / `#[post(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0234`

## Explicit refusals

- Do not invent toolkit/channel productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
