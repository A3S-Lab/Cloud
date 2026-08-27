# 0072: Expose one WorkloadProfile revision management interface

Status: Accepted

## Context

`P0.2-C1` through `C5` define and production-compose the canonical
`a3s.cloud.workload-profile.v1` contract, immutable revision authority,
authorization-first acceptance, exact accepted-profile compilation, and the
sole migration `147` repository transaction. Those capabilities are internal,
so operators and maintained automation cannot yet accept a profile or inspect
its current and historical accepted revisions.

Adding REST, Management MCP, client, and CLI reads independently would create
competing page rules, response projections, repository access, and
authorization behavior. Repository adapters are also outside the trust
boundary: a public read must fail closed when an adapter returns invalid ACL,
foreign scope, a discontinuous sequence, or more rows than requested.

## Decision

`P0.2-C6` adds one `WorkloadProfileQueryService` in Developer Workflows
Application. It depends only on `IWorkloadProfileRepository` and
`IDeveloperWorkflowAuthorizationPort`. The current, exact-revision, and
revision-history query handlers share that service. It:

1. authorizes the exact Organization, Project, Environment, Principal, and
   closed `ReadWorkloadProfile` action before semantic validation or repository
   access;
2. validates the logical profile and optional exact revision identities;
3. revalidates every restored revision, its canonical ACL, and exact requested
   scope; and
4. treats an empty revision history as an absent logical WorkloadProfile; and
5. enforces one `1..=100` list bound and a continuous ascending revision page
   beginning at revision one.

The public interface is a projection over existing CQRS and Domain authority:

- REST accepts one canonical profile ACL bound to an accepted BuildPlan, gets
  the current revision, lists bounded history, and gets one exact revision;
- OpenAPI contract `1.74.0` describes the same closed inputs and fully typed,
  Secret-material-free outputs;
- the maintained TypeScript client and CLI call only those REST operations;
  and
- four Management MCP tools dispatch the same command and queries and reuse
  the REST response DTOs.

Acceptance requires coarse `build:write`; reads require coarse `cloud:read`.
The shared Developer Workflows authorization port remains the exact active
Membership, Resource Grant, and Environment authority on every surface.
Presentation does not import a repository, concrete authorization adapter,
owner context, or ACL parser.

The acceptance request contains only `buildPlanId` and bounded canonical
`profileAcl`. Responses preserve the canonical contract ACL, digest, exact
BuildPlan and SourceRevision evidence, typed profile intent, actor, timestamp,
and immutable revision identity. Secret values, source bytes, credentials,
checkout state, BuildRun state, and downstream owner lifecycle are excluded.
A3S ACL remains the only product configuration language and is parsed only by
the existing Domain contract through `a3s-acl`.

C6 adds no schema, table, migration, aggregate, parser, repository, evaluator,
provider, checkout, Outbox, Relay, queue, worker, retry rail, cache, BuildRun,
Workload, Execution, Route, Operation, timer, scheduler, or cleanup authority.

## Consequences

- REST, Management MCP, client, and CLI share one authorization, validation,
  revision-order, page-bound, and response authority.
- Invalid or cross-scope repository state fails closed before Presentation can
  serialize it.
- WorkloadProfile acceptance remains an immutable intent decision; exposing it
  does not imply compilation or downstream deployment availability.
- Preview public management, pre-acceptance discovery, and owner lifecycle,
  scheduling, route, operation, expiry, and cleanup handoffs remain later P0
  work.
