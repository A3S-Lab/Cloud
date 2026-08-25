# 0061: Project pull-request Preview lifecycle through one consumer authority

Status: Accepted

## Context

Developer Workflows already owns the pure pull-request Preview reducer and
immutable Preview Policy revisions. Sources publishes one exact
Subscription-bound `source.pull-request-change.committed@1` fact through its
authenticated Inbox and the shared transactional Outbox. The missing boundary
was a durable consumer-owned lifecycle projection.

Looking up the current policy when a delayed fact arrives would let Relay delay
rewrite history: a policy accepted after the fact could silently change owner,
quota, fork trust, protected-Secret eligibility, or lifetime. Rebinding an
existing Preview on every fact would have the same defect. A separate Inbox,
queue, retry table, or worker would instead duplicate the platform's existing
Outbox Relay.

## Decision

Component-only `P0.3-C4` gives Developer Workflows one local
`PullRequestPreview` projection authority and one immutable projection receipt
per Sources fact. An anti-corruption projector implements the existing
`IIntegrationEventProjector` contract and translates only the closed Sources
Published Language into the local Application port. Both the all-in-one and
dedicated Relay processes compose that same projector. No second event
consumer, publisher, relay, queue, retry loop, or worker is introduced.

For a new Preview, the Application service selects the latest accepted policy
revision whose `accepted_at` is no later than the Outbox fact's `occurred_at`.
That exact revision becomes immutable lifecycle authority. Later pull-request
facts advance the Preview through the pure provider-time/content reducer but
cannot rebind it to a later policy. Any future policy rebind requires a
separate explicit Developer Workflows reconciliation decision.

The consumer checks an existing receipt before reduction. Reusing an opaque
Sources fact ID with changed content, event time, tenant, Subscription, or PR
binding fails with conflict. Exact replay returns the original decision.
No-applicable-policy and first denied-fork decisions are terminal receipts
without a Preview row. Duplicate and stale lifecycle facts retain the current
Preview version while receiving their own immutable receipt.

Migration `157` stores `developer_pull_request_previews` and
`developer_pull_request_change_projections`. One PostgreSQL transaction takes a
PR-scoped advisory lock, compares the exact observed Preview version, applies
at most one `+1` aggregate mutation, and inserts the receipt. Database foreign
keys bind the Preview to its exact immutable policy revision; triggers reject
authority changes, skipped CAS versions, Preview deletion, and receipt
mutation. The in-memory adapter implements the same atomic contract.

The projection creates no Environment, SourceRevision, BuildRun, Workload,
Deployment, Route, Operation, timer, scheduler, provider delivery, or Secret
material. It does not expose a public API.

## Consequences

- Relay delay cannot select a policy accepted after the owner fact or silently
  rebind an existing Preview.
- Duplicate, stale, reordered, concurrent, and process-restarted delivery
  converges through one reducer, one CAS aggregate, and immutable local
  receipts.
- Projection receipts are consumer decision evidence, not another transport
  Inbox or retry lifecycle; delivery remains exclusively owned by the shared
  Outbox Relay.
- Pull-request Previews remain unavailable until explicit owner interfaces
  compose Projects, Artifacts, Workloads, Edge, Operations, cleanup/expiry, and
  public management without moving those authorities into Developer Workflows.
