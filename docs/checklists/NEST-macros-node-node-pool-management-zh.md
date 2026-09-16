# Nest macros fleet node + node-pool management commands

## Scope

- Fleet node enrollment-token/ready/drain/revoke HTTP adapters migrated to Nest
  attribute macros.
- Fleet node-pool create/member/maintenance HTTP adapters migrated to Nest
  attribute macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[post(..., raw)]`
- [x] scopes metadata (`NODE_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0217` / `0218`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations or `APP0.6` `implemented` as full public release.
