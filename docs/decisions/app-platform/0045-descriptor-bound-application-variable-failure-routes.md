# 0045: Route Application variable failures through descriptor-bound DAG edges

Status: Accepted

## Context

Decision 0041 dispatches descriptor-bound Application conversation-variable
assignments through one Applications-owned snapshot and compare-and-swap port.
Every owner error previously left the write Hook unresolved, even when the
owner had returned a deterministic terminal rejection and the immutable graph
declared an error edge. The admitted descriptor and executable DAG therefore
could not express a bounded Application-variable failure path.

Adding an Applications-specific branch engine, retry rail, error table, or raw
owner-error copy would duplicate existing authorities and could disclose
private session state. Reinterpreting Plan versions 1 through 5 or WorkflowRun
versions 1 through 13 would also break immutable replay behavior.

## Decision

The exact `application.conversation-variable-assign` descriptor may declare one
required, static, single object-valued `error` output, owner-classified retry,
and failure-branch fallback. A graph may select that output through one ordinary
Workflow DAG edge. Capability-free generic Service steps and every other
Applications descriptor remain ineligible.

The compiler emits `cloud.workflow.plan.v6` only when an exact Application
variable assignment selects that descriptor-bound edge. Application-composed
Plan-v6 runs use immutable WorkflowRun input/runtime/Flow version 14 and runtime
build `a3s-cloud-workflows@16`; builds `@1` through `@15` remain explicit replay
entries. Plans v1-v5 and WorkflowRun inputs v1-v13 retain their exact bytes and
behavior.

The existing write Hook remains the sole observation path. Applications
`Invalid`, `NotFound`, `Conflict`, and `Forbidden` results are deterministic
terminal rejections. The coordinator resumes that exact Hook with only the
closed classification and its existing authority-bound identity. Flow converts
it into `cloud.workflow.step-failure.v3`, selects the descriptor's `error`
handle, and lets the ordinary DAG activate the reachable failure branch.
`Unavailable` and `Internal` results remain unresolved coordination failures;
the Hook stays active for the existing idempotent retry path.

Failure v3 stores only the step identity, closed classification, and one stable
classification-specific message. Raw Applications errors, variable values,
session details, and copied owner evidence are excluded. The Applications
repository remains the sole variable and effect authority, and its compare-and-
swap request is unchanged.

Projection reconstructs the same result from immutable WorkflowRun input and
verified Flow history. The source Service remains `failed`, exposes the exact
`error` selected handle and redacted error, and has no successful result. Its
reachable failure sink may complete, allowing the parent WorkflowRun to
complete without converting the rejected assignment into success. Exact replay
does not repeat a terminal owner write.

Migration `123` already admits the failed Service selected-handle projection;
the aggregate still proves Plan v6, the exact Applications descriptor, failure
contract, declared edge, v3 evidence, and selected handle before persistence.
No database or public OpenAPI schema change is required.

## Consequences

- Deterministic Application variable write rejections use the same descriptor,
  Plan, DAG, Flow, and projection mechanisms as other admitted step failures.
- Transient or internal owner failures retain Decision 0041's fail-closed,
  idempotent retry behavior instead of becoming business data.
- Persisted Hook, failure, and projection evidence never includes raw owner
  error text or conversation-variable values.
- This adds no table, column, migration, queue, worker, scheduler, timer, retry
  counter, provider path, second variable store, public route, or configuration
  language.
