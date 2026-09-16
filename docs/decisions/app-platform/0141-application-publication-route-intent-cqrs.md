# 0141. Application publication route intent CQRS

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C12

## Context

`APP0.3-C11` / ADR 0140 persisted Applications-owned
`ApplicationPublicationRouteIntent` behind
`IApplicationPublicationRouteIntentRepository` without authorizing create or
read. Operators still cannot manage exact-release publication channel and
rate-policy profile references through Applications CQRS, while Gateway apply
and Delivery rate middleware remain later and must not invent a second owner.

## Decision

- Add project-authorized Applications CQRS:
  - `CreateApplicationPublicationRouteIntent` reconstructs the exact release,
    fails closed when the caller digest does not match the stored release,
    builds the domain aggregate, and persists via `create_intent`
    (`IdempotentWrite` / deterministic id replay).
  - `GetApplicationPublicationRouteIntent` loads by org/project/application/
    intent id.
  - `ListApplicationPublicationRouteIntentsByRelease` lists by exact release
    identity including digest after verifying the release exists.
- Reuse Applications project write/read access evaluation (`ApplicationAccess`
  / `project`) before mutate or read — same fail-closed shape as sibling
  Applications commands.
- Register handlers on the management CQRS bus and consume the repository
  already wired on `PostgresAdapters` (remove composition dead-code allow).

## Exclusions

- REST / OpenAPI / client / CLI / MCP
- Gateway snapshot publish or Edge PublishRoute apply
- Delivery rate middleware / token bucket
- SSE / browser UI

## Consequences

- Management composition can authorize create/get/list over the existing
  repository without reopening channel vocabulary or inventing Delivery-owned
  rate policy. A later REST/OpenAPI slice can expose the same commands before
  Gateway projection.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib application_publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib cloud_migration_manifest`
