# 0263. Nest-macro Sources GitHub connection callbacks

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Sources GitHub OAuth callback HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Sources GitHub OAuth setup/callback GETs are public routes with an
OAuth no-store error filter on the builder path.

## Decision

1. Convert GitHub connection setup/callback GETs to Nest macros
   (`#[controller("/source-connections")]` / `#[metadata("auth.public", true)]`
   / `#[get(..., raw)]`).
2. Keep `OAuthNoStoreErrorFilter` attached after `.controller()` because filter
   attachment remains wiring-owned.
3. Refuse inventing remaining planned foreign gates and refuse treating Nest
   migrations as public APP0.6 completion.

## Consequences

Adds Nest coverage for Sources GitHub OAuth callback HTTP. OAuth no-store
remains explicit at wiring. Production release remains blocked on honest
remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_github_connection_callbacks_*` lib tests
