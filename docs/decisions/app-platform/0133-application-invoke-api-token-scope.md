# 0133. Admit Identity `application:invoke` on Principal-bound API tokens

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C4

## Context

APP0.3-C1 made exact `ResourceGrantScope::Application` authority real and
reserved `application:invoke` without adding it to Identity bootstrap scopes.
APP0.3-C3 serves anonymous Delivery HTTP only. Rule 8 requires authenticated
published-application callers to use an Identity-issued, Principal-bound
credential plus an exact Application Resource Grant. CreateApiToken already
requires requested scopes ??issuer scopes; without bootstrap admission,
operators cannot mint invoke-bearing tokens.

## Decision

- Add `ApiTokenScope::APPLICATION_INVOKE` to `bootstrap_scopes()` so Principal-
  bound tokens (and interactive credentials derived from bootstrap) may carry
  `application:invoke`.
- Keep the existing split: **scope on token, resource on grant**. Do not couple
  CreateApiToken to ResourceGrant.
- Prove Rule 8 fail-closed evidence via `ResourceAuthorizationDecision`: exact
  Application grant + invoke succeeds; Project grant alone refuses Application
  invoke.

## Exclusions

- Gateway / SSE / browser / embed
- Secrets / Issue Environment material minting
- OpenAPI / MCP / client / CLI surface changes (scopes remain free-form strings)
- Switching management routes from `application:write` to `application:invoke`

## Consequences

- Operators with bootstrap/interactive authority can issue Principal-bound
  tokens that carry `application:invoke`.
- Authorization still requires an exact Application grant (or broader
  membership role) at decision time.
- Authenticated Delivery admission that consumes invoke + Application grant is
  APP0.3-C5 (ADR `0134`).

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib api_token_scope`
- `cargo test -p a3s-cloud-control-plane --lib resource_authorization_decision`
