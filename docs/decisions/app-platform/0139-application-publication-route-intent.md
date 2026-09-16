# 0139. Application publication route intent

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C10

## Context

ROADMAP joint APP0.3 still named Applications-owned exact-release publication
route intent and rate policy as Cloud residual after Delivery process HTTP,
authenticated observation, OpenAPI, drain, and client/CLI (C2?C9). Without a
domain aggregate, later Gateway apply and Delivery HTTP risk inventing local
rate limiters or overfitting SSE/browser transport.

## Decision

- Freeze `ApplicationPublicationRouteIntent` as a component-only Applications
  domain aggregate bound to exact `organization_id`, `project_id`,
  `application_id`, `application_release_id`, and `application_release_digest`.
- Admit a closed publication channel set aligned with parity-manifest
  `publication.*` capabilities, serialized as snake_case:
  `api_blocking`, `api_streaming`, `embed`, `mcp`, `web`, `internal`.
- Require a non-empty unique channel set (`BTreeSet`); reject empty and
  duplicate channel inputs fail-closed.
- Optionally admit a bounded embed origin allowlist: canonicalize http(s)
  origins, reject empty/duplicate/oversized/invalid entries, and require the
  `embed` channel when origins are present.
- Declare an opaque non-empty rate-shaping policy profile id plus exact
  `Sha256Digest` policy revision digest. Gateway enforces versioned burst/rate
  shaping later per `docs/distributed-api-consistency-architecture.md` section 8;
  Applications does not implement Delivery-local token buckets here.
- Derive deterministic UUIDv5 identity from release binding + channels +
  origins + rate profile refs under namespace material
  `application-publication-route-intent:v1`. Exact `create` replays the same id.
- Expose `create` / `restore` / `validate` only. No persistence, CQRS, REST,
  Gateway snapshot publish/apply, or Delivery middleware in this gate.

## Exclusions

- Migration / repository / CQRS / OpenAPI / client / CLI / MCP
- Gateway snapshot publish or apply
- Delivery process rate-limit middleware / token bucket
- SSE / browser UI / shared cursor transport
- Changes to `ApplicationDeliveryPolicy` interaction/response modes

## Consequences

- Later APP0.3 slices can project this intent into Gateway/Delivery without
  reopening channel vocabulary or inventing a second rate-policy owner.
- `ApplicationDeliveryPolicy` remains the experience/response-mode contract;
  publication route intent stays a separate exact-release aggregate.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib application_publication_route_intent`
