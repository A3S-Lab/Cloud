# 0187. Nest-macro GitHub repository subscription query controller

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin GET query
surfaces. `github_repository_subscription_queries_controller` is a tenant-guarded Sources list GET.

## Decision

Convert the controller to Nest macros (`#[controller]` / `#[use_guard]` /
`#[get(..., raw)]`, plus scopes metadata when previously present). Keep the
factory for module registration. Refuse inventing `S0` BYOK and refuse marking
Nest migrations as full production release.

## Consequences

Adds Nest query precedent. Production release remains blocked on honest
remaining gates (notably unavailable `S0` BYOK / non-public `APP0.6`).

## Evidence

- This ADR
- Focused `nest_macro_*` lib tests
