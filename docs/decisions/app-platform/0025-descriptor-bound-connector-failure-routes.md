# 0025: Route Connector failures through descriptor-bound DAG edges

Status: Accepted

## Context

Decision 0019 routes finite-Execution failures through the ordinary DAG edge
declared by an immutable step descriptor. Decisions 0020 through 0022 keep
Connector attempt, retry, response-object, and read authority with Flow and
Connectors. Decision 0024 then projects an accepted response through one
schema-bound no-retry Flow step. Connector rejection, exhausted attempts,
indeterminate dispatch, exhausted observation, and invalid response content
still terminated the parent run even when the admitted `connector.http`
descriptor and graph declared an exact error output.

Adding a Connector-specific branch engine, retry rail, error table, or evidence
copy would duplicate existing authorities. Reinterpreting Plan v2 through v4 or
WorkflowRun versions 1 through 8 would also break immutable replay behavior.

## Decision

A graph with a `Service` step bound to one exact `ConnectorRevision` may select
the error output declared by its immutable descriptor failure contract. The
edge remains an ordinary DAG edge and must be the single required static
object-valued failure output. The compiler emits `cloud.workflow.plan.v5` with
compiler revision `cloud.workflow.plan-compiler.v5`. Plan versions 1 through 4
continue to reject Connector failure routes.

New Plan-v5 runs use WorkflowRun input, runtime contract, and Flow version 9
and replay build `a3s-cloud-workflows@9`. Runtime builds `@1` through `@8`
remain explicit replay-compatible generations. Version 9 preserves the exact
Connector hook, attempt, wait, response-object, and typed response behavior of
version 8.

Flow classifies terminal Connector outcomes as provider rejection, exhausted
attempts, indeterminate dispatch, exhausted observation, or invalid response.
Only after finding the exact descriptor-bound edge does it materialize a
bounded `cloud.workflow.step-failure.v2` value containing the step identity,
closed classification, and sanitized message. Raw provider bytes, HTTP
details, credentials, and copied C6 evidence are excluded; the exact evidence
and response object remain in their existing authorities.

The same `WorkflowLocalStepResult` and dependency matcher used by branches and
finite-Execution failure routes selects the Connector error handle. A typed
response step remains no-retry; when an exact failure route exists, its Flow
failure policy permits the parent workflow to interpret that terminal error.
It does not authorize another object read or provider attempt. Without the
exact route, version-8 fail-closed behavior remains unchanged.

Projection reconstructs the same typed result from immutable WorkflowRun input
and verified Flow history. The Connector step remains `failed` with its exact
selected handle and bounded error, the reachable failure sink may complete,
and the parent may complete. No failure value becomes a successful Connector
result.

Migration `123` widens only the existing Workflow step-projection kind and
selected-handle check constraints so the already wired Service/Connector shape
can be persisted. Those structural checks do not authorize a generic Service:
the WorkflowRun aggregate still proves the immutable ConnectorRevision binding,
descriptor failure contract, declared edge, failed status, and exact handle
before either repository writes the projection.

## Consequences

Connector errors now use the same descriptor, Plan, DAG, Flow, and projection
mechanisms as other admitted step failures. Historical Plan and WorkflowRun
bytes keep their behavior, while version 9 composes the new interpretation
explicitly.

This adds one constraint-only migration, but no table, column, queue, worker,
scheduler, timer, retry counter, child Operation, provider client, object
namespace, credential authority, evidence store, public response-body surface,
or configuration language. It does not make HTTP Request or `AUT0.5` publicly
available; remaining provider, recovery, retained integration, interface, and
other typed-step gates still apply.
