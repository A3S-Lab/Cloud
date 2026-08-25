# 0063: Preview Environment handoff through the existing owner authorities

Status: Accepted

## Context

ADR 0061 gives Developer Workflows one durable, policy-revision-bound Preview
projection. A Preview also derives one stable ordinary `EnvironmentId`, but
Developer Workflows does not own the Projects `Environment` aggregate, its
name uniqueness, persistence, idempotency, or events. Creating that row inside
the Preview repository would duplicate Projects authority. A Preview-specific
saga, queue, relay, retry table, or worker would likewise duplicate the shared
transactional Outbox and Relay.

The owner handoff must remain replayable after process death. Reading the
mutable Preview row from a downstream consumer would make delivery depend on
current local state rather than the exact aggregate version that committed the
request.

## Decision

Component-only `P0.3-C5a` publishes one
`developer.pull-request-preview.lifecycle-committed@1` fact whenever, and only
whenever, the Preview aggregate advances. The Preview row, immutable Sources
fact receipt, and lifecycle Outbox event share the existing Developer
Workflows PostgreSQL transaction. Its immutable wire type physically belongs
to Developer Workflows `published`; aggregate reconstruction and event
generation stay in Domain. The fact carries the exact Preview version,
policy authority, deterministic Environment identity and name, PR/source
evidence, trust decision, quotas, lifecycle state, correlation, and causation.
It contains no credential, provider delivery identity, raw webhook body, or
foreign aggregate.

The existing `PullRequestPreviewProjector` remains the context's sole
`IIntegrationEventProjector`. The same shared Outbox Relay delivers both the
Sources fact into Developer Workflows and the resulting Preview lifecycle fact
to the owner handoff. No new Inbox, publisher, subscriber, queue, retry loop,
timer, worker, or saga is introduced.

Developer Workflows Application owns `IPreviewEnvironmentPort` and the minimal
`PreviewEnvironmentBinding`; it imports no Projects model. One Infrastructure
anti-corruption adapter translates that request into Projects' existing
`Environment`, `IEnvironmentRepository`, idempotency, transactional Outbox,
and `project.environment.created` event. The production projector requires the
port at construction, so an active lifecycle cannot run with an incomplete
composition. All-in-one and dedicated Relay roles use the same composition
function and PostgreSQL adapter family.

An active lifecycle ensures the one deterministic ordinary Environment. Exact
replay, including a concurrent unique-key race, returns the existing exact
binding without publishing another Projects event. A preclaimed identity or
name with different tenant, Project, name, version, or creation time conflicts.
Cleanup-required lifecycle facts deliberately create nothing: Projects has no
archive/delete lifecycle in this slice, and C5a cannot invent one.
Environment creation is monotonic and therefore converges even if an older
active fact arrives after a newer cleanup fact. Later executable owner
handoffs such as build, deployment, and traffic must add their own
aggregate-version consumer fence before performing reversible lifecycle work;
C5a's creation-only rule is not authority to ignore ordering there.

## Consequences

- Developer Workflows retains Preview policy and lifecycle authority; Projects
  remains the sole Environment aggregate and persistence authority.
- One committed Preview mutation produces at most one lifecycle fact, and one
  logical Preview produces exactly one ordinary Projects Environment.
- Transaction rollback cannot leave a mutated Preview without its lifecycle
  event; Relay replay and process restart converge without a second delivery
  mechanism.
- Architecture tests confine Projects model imports to the single
  Infrastructure adapter and reject optional production composition or another
  Developer Workflows projector/relay mechanism.
- Pull-request Previews remain unavailable. SourceRevision/build candidate,
  BuildRun, Workload/Deployment, Route, Operation, expiry/cleanup execution,
  Secret materialization, and management interfaces remain explicit later
  owner slices.
