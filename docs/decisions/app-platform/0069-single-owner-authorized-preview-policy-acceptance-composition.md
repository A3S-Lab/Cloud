# 0069: Compose one owner-authorized Preview Policy acceptance path

Status: Accepted

## Context

`P0.3-C2` defines the canonical Preview Policy ACL, authorization-first
acceptance command, consumer-owned source-subscription query port, immutable
revision authority, and migration `153`'s atomic revision, idempotency, audit,
and Outbox write. The command still had no production CQRS registration, the
query port had no production Sources adapter, and API/Worker composition did
not select the existing policy repository.

BuildPlan and workload-profile acceptance already share one production
Developer Workflows authorization port over Identity Membership and Resource
Grant interfaces, Identity's sole `ResourceAccessEvaluator`, and the exact
Projects Environment. Relay already selects the existing Preview Policy
repository to resolve event-time policy while projecting lifecycle facts.
Creating another authorization evaluator, subscription model, policy store, or
role-crossing repository instance would duplicate an existing authority.

## Decision

`P0.3-C6` production-composes exactly one internal
`AcceptPullRequestPreviewPolicy` command on the existing CQRS bus. The handler
shares the exact `Arc<dyn IDeveloperWorkflowAuthorizationPort>` already used by
BuildPlan and workload-profile acceptance.

One `RepositoryPreviewSourceSubscriptionQueryPort` in Developer Workflows
Infrastructure adapts Sources' existing `ISourceSubscriptionRepository`. It
performs the exact Organization/subscription lookup, delegates stored aggregate
validation to `GithubRepositorySubscription::restore`, rejects a returned
identity outside the requested scope, and maps only the consumer-owned binding:
Organization, Project, source Environment, subscription, GitHub installation,
canonical repository, canonical branch, and active state. Connection, recipe,
credential, webhook Inbox, and revision internals do not cross the port.

The typed PostgreSQL factory grants API/Worker one writer/read instance and
Relay its own read instance of the existing
`PostgresPullRequestPreviewPolicyRepository`. Both role families select it
through one concrete-constructor rule, so neither process receives the other
role's repository family. Migration `153` remains the sole policy revision,
idempotency, audit, and Outbox transaction.

Authorization still precedes ACL parsing, replay, Sources lookup, and policy
persistence. The existing handler validates the exact active subscription,
canonical policy binding, continuous revision, and semantic convergence before
delegating the only write to the repository.

C6 adds no public route, client, CLI, Management MCP tool, table, migration,
authorization evaluator, Sources aggregate, subscription lifecycle, event
rail, Inbox, relay, queue, worker, Preview lifecycle mutation, Environment,
SourceRevision, BuildRun, Workload, Execution, Route, Operation, retry, timer,
scheduler, or cleanup authority.

## Consequences

- Production has one authorization-first Preview Policy acceptance command and
  one exact Sources subscription anti-corruption boundary.
- All three Developer Workflows acceptance commands use the same Identity and
  Projects authorization mechanism.
- Management and Relay use separate role-scoped repository instances without
  duplicating the concrete repository-construction rule or policy authority.
- Architecture and composition tests freeze Application isolation, the two
  allowed Sources query adapters, shared authorization, role separation, and
  exactly-once CQRS registration.
- Public policy management and all Preview resource/cleanup interfaces remain
  later explicit slices.
