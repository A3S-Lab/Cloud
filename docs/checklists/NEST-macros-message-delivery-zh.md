# Nest macros application message delivery controllers

## Scope

- Message variant / citation / file-reference delivery HTTP adapters migrated to
  Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[metadata]` / `#[get|post(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0227` / `0228` / `0229`

## Explicit refusals

- Do not invent deferred toolkit/channel surfaces here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
