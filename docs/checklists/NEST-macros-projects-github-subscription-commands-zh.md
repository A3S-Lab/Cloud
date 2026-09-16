# Nest macros project commands + GitHub subscription commands

## Scope

- Project create/attribution and environment create HTTP adapters migrated to
  Nest attribute macros.
- GitHub repository subscription create/deactivate HTTP adapters migrated to
  Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[post(..., raw)]`
- [x] scopes metadata (`PROJECT_WRITE` / `ENVIRONMENT_WRITE` / `SOURCE_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0215` / `0216`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
