# Nest macros privileged management controllers

## Scope

- Platform RBAC, workload trust, and tenant-support controllers migrated to Nest attribute macros.

## Done

- [x] Nest `#[controller("/platform")]` / `#[metadata("auth.scopes", ...)]` / `#[post|get(..., raw)]`
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0241`

## Explicit refusals

- Do not invent SAML/SCIM/SIEM or BYOK productization here.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.

