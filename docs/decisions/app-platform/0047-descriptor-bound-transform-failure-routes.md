# 0047: Route Workflow-local Transform failures through descriptor-bound DAG edges

Status: Accepted

## Context

Workflow-local Transform steps execute deterministic template evaluation inside
the existing Flow step runtime. A missing token, invalid projected input, or
output-schema mismatch previously exhausted the local step and failed the
entire WorkflowRun even when the immutable Transform descriptor and graph
declared an `error` output.

Copying the evaluator's raw error into graph data would disclose template or
input details. Retrying a deterministic invalid evaluation would add work
without changing the result. Reinterpreting Plan versions 1 through 7 or
WorkflowRun versions 1 through 15 would also break immutable replay behavior.

## Decision

An admitted Workflow-owned Transform descriptor may declare one required,
static, single object-valued `error` output, non-retryable classification, and
failure-branch fallback. A graph may select that exact output through one
ordinary DAG edge while retaining its normal output edge. Revision semantic
validation remains the authority for the exact descriptor, failure contract,
and selected handle.

The compiler emits `cloud.workflow.plan.v8` when at least one Transform selects
that descriptor-bound edge. The resulting immutable WorkflowRun uses
input/runtime/Flow version 16 and runtime build `a3s-cloud-workflows@18`;
builds `@1` through `@17` remain explicit replay entries. Plans v1-v7 and
WorkflowRun inputs v1-v15 retain their exact bytes and behavior. Plan v8 is
cumulative and may preserve independently admitted Execution, Connector, and
Application failure routes.

Runtime v16 schedules the routed Transform once with Flow's existing
continue-on-failure action and no retry. On replay, it derives a bounded
`cloud.workflow.step-failure.v5` value with classification
`workflow_local_invalid`, one fixed redacted message, no details, and the exact
step identity. It selects the descriptor's `error` handle and lets ordinary DAG
reachability activate the failure sink. Raw evaluator errors never become DAG
data or public projection errors.

Projection reconstructs the same typed result from immutable WorkflowRun input
and verified Flow history. The Transform remains `failed`, exposes the exact
selected handle and fixed redacted error, and has no successful result. Its
failure sink may still complete the parent WorkflowRun.

Migration `145` only widens the existing selected-handle constraint to admit a
failed Transform projection. Aggregate validation still proves Plan v8, the
exact descriptor failure contract, the declared edge, failure-v5 shape, and
selected handle before persistence. No public REST shape changes because the
existing plan schema and typed projection fields already carry these values;
the maintained TypeScript client now enumerates Plan v5-v8 and failure v2-v5.

## Consequences

- Deterministic Transform evaluation failures use the same descriptor, Plan,
  DAG, Flow, and projection mechanisms as other admitted step failures.
- A routed local failure executes exactly once and cannot leak raw evaluator
  diagnostics through handled data or the public step projection.
- Historical Plans, Run inputs, and runtime builds keep exact replay behavior.
- This adds no table, column, queue, worker, scheduler, timer, retry engine,
  provider path, public route, or second orchestration authority.
