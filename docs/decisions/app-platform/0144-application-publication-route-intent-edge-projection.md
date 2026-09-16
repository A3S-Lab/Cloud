# 0144. Application publication route intent Edge projection

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C15

## Context

APP0.3-C10 through C14 freeze, persist, authorize, and expose
`ApplicationPublicationRouteIntent` through management REST/OpenAPI and
cloud-client/CLI. Gateway still needs an Applications-owned Edge-facing
desired-state shape before PublishRoute admission or snapshot compile can
consume exact-release channels, embed origins, and rate-shaping profile refs.
Edge `PublishRoute` today admits workload hostname/path routes only; inventing
a live apply call here would either misuse that command or invent Delivery rate
middleware.

## Decision

- Freeze `ApplicationPublicationRouteIntentEdgeProjection` as the aggregate-free
  Applications consumer DTO for Edge desired state: exact org/project/application
  and release identities, intent id, ordered snake_case channels, embed origin
  allowlist, and opaque rate-shaping profile id plus policy revision digest.
- Add a pure Applications infrastructure ACA mapper
  `project_application_publication_route_intent_edge` that validates the intent,
  projects fields, and validates the projection. The mapper does not import Edge
  command or aggregate types.
- Do not call `PublishRouteHandler`, `GatewaySnapshotCompiler`, or node install.
- Do not add REST/OpenAPI, CQRS emit ports, Delivery rate middleware, SSE, or
  browser surfaces in this gate.

## Exclusions

- Edge `PublishRoute` admission / Gateway snapshot compile / node install
- CQRS command that persists intent and emits projection through a port
- REST/OpenAPI bump, client/CLI/MCP changes
- Delivery process rate-limit middleware / token bucket
- SSE / browser UI

## Consequences

- Later APP0.3 slices can admit this projection into Edge PublishRoute / snapshot
  compile without reopening channel or rate-policy vocabulary.
- Rate shaping remains declare-only until Gateway enforces the versioned profile
  per `docs/distributed-api-consistency-architecture.md` section 8.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib publication_route_intent_edge_projection`
- `cargo test -p a3s-cloud-control-plane --lib publication_route_intent`
