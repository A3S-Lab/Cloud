# Nest macros organization-scoped application delivery controllers

## Scope

- Organization-scoped application delivery commands/queries migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard(OrganizationTenantGuard)]` / `#[metadata("auth.scopes", ...)]` / `#[post|get(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0236`

## Explicit refusals

- Do not invent toolkit/channel productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.

