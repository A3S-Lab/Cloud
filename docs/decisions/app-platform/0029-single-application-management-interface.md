# 0029: Expose one Application management authority

Status: Accepted

## Context

`APP0.1-C1/C2/C3` established one immutable Applications-owned release
contract, one PostgreSQL transaction boundary, and authorization-before-replay
CQRS. The first maintained management interfaces must expose that authority
without creating presentation-local state, accepting mutable Workflow heads,
or weakening Resource Grant and idempotency behavior.

Separate REST, client, CLI, or MCP implementations of publication admission
would allow authorization order, Workflow evidence, concurrency, replay, and
response contracts to drift. Resolving a Workflow's current revision at read or
replay time would also break the exact immutable binding.

## Decision

Applications defines two commands and four queries:

- create an Application with its first immutable release;
- publish a version-checked successor release;
- list or get Application heads; and
- list release history or get one exact immutable release.

Every command and query authorizes the project through the shared Resource
Grant evaluator. Mutations authorize before looking up idempotency replay, so a
later grant revocation takes effect on the next request. New writes use one
adapter over the existing Workflow repository to load the exact named revision
and match its contract, payload-set, semantic-contract-set, input-schema, and
output-schema evidence. A valid replay returns the persisted historic result
without re-reading Workflow authority.

REST/OpenAPI `1.42.0`, the maintained TypeScript client, CLI, and six Management
MCP tools are thin adapters over those handlers and response DTOs. Writes use
the dedicated `application:write` API-token scope; reads use `cloud:read`.
Denied project access and missing Application/release identities retain the
shared fail-closed response contract.

One crate-level request context owns the identical request-ID, idempotency-key,
and authenticated-actor extraction used by Applications and compatible
controller modules. Domain-specific ACL, expected-version, credential-actor,
and authorization rules remain with their owning modules.

This interface adds no session, invocation, message, graph, Plan, Flow history,
provider execution, Secret material, Gateway route, worker, queue, or second
repository. Those capabilities remain owned by later APP0 gates and their
existing contexts.

## Consequences

- All maintained management surfaces observe one authorization, concurrency,
  evidence, idempotency, persistence, audit, and Outbox authority.
- Exact release reads remain stable after the Application head or Workflow head
  advances.
- Revoked access cannot be recovered through an old idempotency key.
- Completing `APP0.1` is not an Application delivery or product-availability
  claim; `APP0.2` through `APP0.6` remain required.
