# 0264. Nest-macro Identity OIDC controllers

- Status: Accepted
- Date: 2026-09-16
- Gate: platform-control-plane / Identity OIDC federation HTTP

## Context

Nest-style Boot controller macros (ADR `0179`) are preferred for thin HTTP
surfaces. Identity OIDC public login/callback and tenant link begin already use
public / org-tenant guards with an OAuth no-store error filter on the builder
path.

## Decision

1. Convert OIDC public GETs and tenant link POST to Nest macros
   (`#[controller]` / `#[metadata]` / `#[use_guard]` / `#[get|post(..., raw)]`).
2. Keep `OAuthNoStoreErrorFilter` attached after `.controller()` because filter
   attachment remains wiring-owned.
3. Preserve existing OIDC federation claim-path routes; refuse inventing
   SAML/SCIM product surfaces or treating Nest migrations as public APP0.6
   completion.

## Consequences

Adds Nest coverage for Identity OIDC HTTP. OAuth no-store remains explicit at
wiring. Production release remains blocked on honest remaining gates.

## Evidence

- This ADR
- Focused `nest_macro_oidc_*` lib tests
- Existing `enterprise_saml_oidc_scim_claim_path_tests`
