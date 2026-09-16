# 0142. Application publication route intent REST

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C13

## Context

`APP0.3-C12` / ADR 0141 registered project-authorized create/get/list-by-release
CQRS for Applications-owned `ApplicationPublicationRouteIntent` on the
management bus. Operators still lack a management REST/OpenAPI surface before
Gateway Edge projection and Delivery rate middleware.

## Decision

- Expose thin Applications presentation controllers that only dispatch C12
  commands and queries:
  - `POST .../applications/{application_id}/releases/{release_id}/publication-route-intents`
    with body digest, channels, optional embed origin allowlist, and rate-shaping
    policy profile reference (`APPLICATION_WRITE`).
  - `GET .../applications/{application_id}/publication-route-intents/{intent_id}`
    (`CLOUD_READ`).
  - `GET .../applications/{application_id}/releases/{release_id}/publication-route-intents?applicationReleaseDigest=...`
    for exact-release list (`CLOUD_READ`).
- Keep create outside Idempotency-Key requirements because identity is
  deterministic (same posture as delivery-credential registration).
- Bump the committed OpenAPI contract to `1.111.0`.

## Exclusions

- cloud-client / CLI / MCP
- Gateway snapshot publish or Edge PublishRoute apply
- Delivery process routes or rate middleware
- SSE / browser UI

## Consequences

- Operators can manage exact-release publication route intents through the
  management API over the frozen C12 CQRS surface. Client/CLI and Gateway Edge
  projection remain later slices.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib app::tests::api_contract_tests::committed_openapi_snapshot_matches_the_resolved_route_contract -- --exact`
- Focused controller route registration test in
  `publication_route_intent_controller`
