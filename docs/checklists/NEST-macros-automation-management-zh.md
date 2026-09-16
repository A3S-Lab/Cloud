# Nest macros automation management controllers

## Scope

- Automation webhook lifecycle commands and management queries migrated to Nest
  attribute macros with existing tenant helper wraps.

## Done

- [x] Nest `#[controller]` / `#[get|post(..., raw)]`
- [x] Presentation helper wraps retained
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0233`

## Explicit refusals

- Do not invent automation ops console productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
