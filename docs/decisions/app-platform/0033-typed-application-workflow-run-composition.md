# 0033: Compose Application invocations into ordinary WorkflowRuns

Status: Accepted

Decision 0035 supersedes this decision's transient composition fields. The
deterministic identities, typed port, ordinary Workflow records, and Flow
ownership defined here remain accepted.

## Context

Decisions 0031 and 0032 established and persisted the Applications-owned
session and invocation correlation authority. An invocation could reference at
most one `WorkflowRun`, but no production component created that run. A public
delivery layer that called A3S Flow, a provider, or a queue directly would
bypass Workflow's Goal, Plan, run, cancellation, and recovery authorities.

Composition must also survive process death between creating Workflow records
and binding the Applications invocation. Retrying with changed release,
ontology, input, Principal, environment, or timeout authority must not adopt an
unrelated run, and an invocation cancellation that wins the binding race must
not leave unowned execution running.

## Decision

Applications defines one typed internal request/evidence port. The request is
constructed only from the exact immutable `ApplicationRelease`, its
release-pinned `ApplicationSession`, the persisted `ApplicationInvocation`, and
the immutable execution authority added by Decision 0035. It carries canonical
digests rather than copied Workflow or Ontology payloads.

The Applications aggregate identity deterministically derives one `WorkflowRun`
ID: the Organization is the UUID namespace and the versioned name contains the
Application plus invocation IDs. This matches the persisted invocation primary
key and prevents two Applications that reuse one invocation UUID from claiming
the same Organization-scoped WorkflowRun. That run ID in turn derives one
`WorkflowGoal` ID and one `PlanRevision` ID. Repository idempotency scopes also
include the exact Project and Application, while stable request IDs and bodies
include the full canonical authority, so an exact retry adopts committed
records while changed reuse conflicts.

The production adapter loads the exact Workflow definition/revision and
Ontology revision from their existing repositories, validates all release and
revision evidence, and invokes the existing `WorkflowPlanCompiler` and
`WorkflowRunCompiler`. It creates or adopts the ordinary Workflow Goal, Plan,
and Run through the existing Workflow repositories. The existing
`WorkflowRunReconciler`, Operation/Outbox path, and A3S Flow runtime remain the
only execution dispatch and recovery path; Applications does not call Flow, a
provider, or a queue.

An internal CQRS command starts or adopts that typed Workflow run and then
optimistically binds its exact ID to the invocation. Exact replay returns the
stored binding. If a concurrent Applications cancellation commits before the
binding, the handler requests cancellation through the ordinary WorkflowRun
state machine and repository before reporting the conflict. No second
cancellation queue or execution record is introduced.

The production process registers this internal command with the PostgreSQL
Application, Workflow, and Ontology adapters. A PostgreSQL 17 gate reconstructs
all adapters, adopts the same deterministic evidence after restart, rejects
persisted authority drift, and proves that only one Goal, Plan, and Run exist.

## Consequences

- Application invocation composition is typed, digest-bound, deterministic,
  and recoverable across process restart.
- Workflow remains the sole Goal, Plan, WorkflowRun, cancellation, and graph
  authority, while A3S Flow remains the sole durable execution and history
  authority.
- Applications stores its run correlation plus Decision 0035's minimal
  immutable composition authority and never duplicates Workflow or Flow state.
- `APP0.2-C3` is component-only. It intentionally has no HTTP, client, CLI, MCP,
  or channel delivery entry point. Public authorization, session/invocation
  commands, blocking and streaming delivery, public cancellation/replay,
  remaining message/file/feedback records, and retained delivery recovery
  evidence are still required before `APP0.2` is available.
