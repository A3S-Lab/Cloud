# Nest macros automation and GitHub webhooks controllers

## Scope

- Public Automation and GitHub webhook HTTP adapters migrated to Nest attribute
  macros.

## Done

- [x] Nest `#[controller]` / `#[metadata(auth.public)]` / `#[post(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0223` / `0224`

## Explicit refusals

- Do not invent HA/ops console or OAuth productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
