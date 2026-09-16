# 0198. Nest-macro audit query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin GET query
surfaces. `audit_query_controller` is an administrator-scoped Audit list/export
GET surface with two guards (tenant + organization administrator).

## Decision

Convert the controller to Nest macros with dual `#[use_guard]` matching
`usage_retention_controller`, plus scopes metadata and `raw` handlers. Keep the
factory for module registration. Refuse inventing `S0` BYOK and refuse marking
Nest migrations as full production release.

## Consequences

Adds Nest query precedent for dual-guard admin reads. Production release remains
blocked on honest remaining gates (notably unavailable `S0` BYOK / non-public
`APP0.6`).

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
