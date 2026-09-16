# 0140. Application publication route intent persistence

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C11

## Context

`APP0.3-C10` / ADR 0139 froze Applications-owned `ApplicationPublicationRouteIntent`
as a component-only domain aggregate. Exact-release Gateway apply and Delivery
rate middleware still need a durable owner without inventing CQRS, REST, or a
second rate-policy authority.

## Decision

- Persist immutable rows in `application_publication_route_intents` through
  migration `210`.
- Keep idempotency on primary key identity only (deterministic UUIDv5 from C10).
  Multiple channel/origin/rate-profile combinations may exist for one exact
  release as distinct ids; `list_intents_by_release` returns them sorted by id.
- Fence exact release binding with foreign keys to `application_releases`,
  including digest via the existing `(organization_id, application_id, id,
  contract_digest)` unique index.
- Store closed channel vocabulary as `text[]` with cardinality and membership
  checks; store embed origins as `text[]` defaulting to empty; omit `created_at`
  because the domain aggregate has no audit timestamp.
- Provide `IApplicationPublicationRouteIntentRepository` with
  `create_intent` (`IdempotentWrite`), `find_intent`, and
  `list_intents_by_release`, plus PostgreSQL and in-memory adapters.
- Wire the PostgreSQL adapter into `PostgresAdapters` for composition
  consistency without registering CQRS handlers.

## Exclusions

- CQRS commands/queries/handlers
- REST / OpenAPI / client / CLI / MCP
- Gateway snapshot publish or apply
- Delivery rate middleware / token bucket
- SSE / browser UI

## Consequences

- Later APP0.3 slices can authorize create/get/list over this repository and
  project intents into Gateway without reopening channel vocabulary or
  inventing a second persistence owner.

## Evidence

- `cargo test -p a3s-cloud-control-plane --lib application_publication_route_intent`
- `cargo test -p a3s-cloud-control-plane --lib migration_210_persists_immutable_publication_route_intents`
- `cargo test -p a3s-cloud-control-plane --lib publication_route_intent_in_memory`