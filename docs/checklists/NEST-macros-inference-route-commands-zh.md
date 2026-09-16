# Nest macros inference route commands

## Scope

- Inference route publish/revise/retire HTTP adapters migrated to Nest macros.

## Done

- [x] Nest `#[controller]` / `#[use_guard]` / `#[post(..., raw)]`
- [x] scopes metadata (`INFERENCE_WRITE`)
- [x] focused lib `nest_macro_*` tests
- [x] ADR `0207`

## Explicit refusals

- Do not invent `S0` / BYOK.
- Do not treat Nest migrations as full public production release.
