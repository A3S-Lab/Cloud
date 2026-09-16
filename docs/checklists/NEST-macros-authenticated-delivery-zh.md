# Nest macros authenticated application delivery controller

## Scope

- Authenticated delivery admission HTTP adapter migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard(OrganizationTenantGuard)]` / `#[metadata("auth.scopes", ...)]` / `#[post(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0235`

## Explicit refusals

- Do not invent toolkit/channel productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.

