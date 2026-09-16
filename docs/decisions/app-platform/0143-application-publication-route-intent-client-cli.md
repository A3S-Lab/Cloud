# 0143. Application publication route intent client and CLI

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C14

## Context

APP0.3-C13 shipped management REST/OpenAPI for exact-release
`ApplicationPublicationRouteIntent` create, get, and list-by-release. Operators
and SDKs still lacked a maintained TypeScript client and CLI for those routes.
Management MCP already exposes project-member Application tools; inventing MCP
tools for publication route intents would create a second admission/management
path without a distinct operator need (same rationale as APP0.3-C9 / ADR 0138).

## Decision

- Add `CloudApi` methods that call the existing C13 REST paths only:
  create under release scope, get by intent id, and list by exact release digest
  query.
- Keep request/response shapes camelCase and validate channels, digests, and
  rate-shaping policy refs in `packages/cloud-client`.
- Create uses deterministic identity server-side; do not send Idempotency-Key.
- Add CLI commands `application-publication-route-intents create|get|list`.
- Skip Management MCP for this gate.
- Leave `CLOUD_API_CONTRACT_VERSION` lagging OpenAPI `1.111.0` when that matches
  the C9 local pattern for management client gates.

## Exclusions

- Management MCP tools
- Gateway Edge apply / PublishRoute projection
- Delivery rate middleware or Delivery process routes
- SSE / browser
- Control-plane domain, CQRS, or REST/OpenAPI changes

## Consequences

- Clients and CLI can declare publication route intents through the same
  management OpenAPI surface C13 already documents.
- Gateway still enforces rate shaping later from declared policy profile refs.

## Evidence

- `bun test` in `packages/cloud-client` filtered to publication route intent paths
- `bun test` in `cli` filtered to publication route intents
