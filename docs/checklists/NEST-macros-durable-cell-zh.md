# Nest macros durable cell controllers

## Scope

- Durable cell commands, route publish, and queries migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard(OrganizationTenantGuard)]` / `#[metadata("auth.scopes", ...)]` / `#[post|get(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0237`

## Explicit refusals

- Do not invent HA/ops console productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.

