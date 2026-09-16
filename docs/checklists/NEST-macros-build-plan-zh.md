# Nest macros build plan controllers

## Scope

- Developer Workflows build-plan HTTP adapters migrated to Nest attribute macros
  with root Presentation tenant-scope helpers.

## Done

- [x] Nest `#[controller]` / `#[get|post(..., raw)]`
- [x] helper wraps for build-write / cloud-read scopes
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0225`

## Explicit refusals

- Do not invent deferred toolkit/channel surfaces here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
