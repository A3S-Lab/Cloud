# Nest macros application feedback, delivery-credential, and publication-route-intent controllers

## Scope

- Feedback/annotation, delivery-credential, and publication-route-intent HTTP
  adapters migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[metadata]` / `#[get|post(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0230` / `0231` / `0232`

## Explicit refusals

- Do not invent secrets CMS, SIEM, or Edge rate middleware productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
