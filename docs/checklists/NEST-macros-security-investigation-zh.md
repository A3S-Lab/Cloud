# Nest macros security investigation controller

## Scope

- Security gateway-route timeline HTTP adapter migrated to Nest attribute macros
  with root Presentation administrator-read composition.

## Done

- [x] Nest `#[controller]` / `#[get(..., raw)]`
- [x] `organization_administrator_read_controller` wrap (no Identity imports)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0222`

## Explicit refusals

- Do not invent SIEM / PII CMS productization.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
