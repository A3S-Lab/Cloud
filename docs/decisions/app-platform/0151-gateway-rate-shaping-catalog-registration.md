# 0151. Cloud Edge Gateway rate-shaping catalog registration

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C22

## Context

APP0.3-C20 introduced Edge compile-time admission against an in-memory Gateway
rate-shaping catalog and emitted bound ACL scalars. Production compilers still
constructed `GatewaySnapshotCompiler::new` with the **empty** admission adapter,
so any declare-only rate ref continued to fail closed even after operators had
no process-local way to register profiles into the catalog the compilers use.

APP0.3-C21 made Gateway apply bound ACL on the node. HTTP request-path admit and
exact-release route attachment remain separate residuals. Delivery must not grow
rate-limit middleware. REST/OpenAPI catalog CRUD remains deferred.

## Decision

1. Expose `register` on `IGatewayRateShapingProfileCatalog` (process-local insert
   or replace of one validated `GatewayRateShapingProfile` revision).
2. Wire one shared in-memory catalog into both production compile sites:
   - api-worker `deployment_route_compiler`
   - management `route_compiler`
   via `EdgeApplicationPublicationRateShapingBindingAdmissionAdapter` so register
   and compile admission observe the same map inside each process.
3. Add component-only CQRS `RegisterGatewayRateShapingProfile` on the management
   application (no REST/OpenAPI/client/CLI in this gate).
4. Keep empty-start fail-closed honesty: processes still boot with an empty
   catalog until something registers profiles.

## Explicit non-owners

- Durable / cross-process catalog persistence or config ACL seed bootstrap
- REST/OpenAPI/client/CLI catalog management
- Delivery process rate-limit middleware
- HTTP request-path admit on Gateway
- Exact-release publication routing / rollback / inventing `PublishRoute` as
  channel owner
- SSE / browser UI / `McpRoutePolicy` conflation

## Consequences

- Operators and tests can register profiles into the catalog that production
  compilers consult within the same process.
- Cross-process seed (api-worker vs management) and durable authority remain
  explicit residuals before rate-shaped publication is fully production-ready.
- Next first-principles residuals stay: durable/config catalog seed; Gateway HTTP
  admit after a real route binding; exact-release routing/rollback.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib -- rate_shaping`
- `cargo test -p a3s-cloud-control-plane --lib -- register_gateway_rate_shaping`
- `cargo test -p a3s-cloud-control-plane --lib -- edge_gateway_rate_shaping_binding`
