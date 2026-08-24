# 0048: Route Workflow-local Output failures through descriptor-bound DAG edges

Status: Accepted

## Context

Workflow-local Output steps render optional templates and validate the resulting
value against the immutable output schema inside the existing Flow step runtime.
A missing token, invalid projected input, or schema mismatch previously failed
the entire WorkflowRun even when the exact `workflow.output` descriptor and graph
declared an `error` output.

Copying evaluator diagnostics into graph data could disclose templates or input
values. Retrying the same deterministic invalid evaluation would add work without
changing the result. Reinterpreting Plans 1 through 8 or WorkflowRun versions 1
through 16 would also break immutable replay behavior.

## Decision

The admitted Workflow-owned `workflow.output` descriptor may declare one
required, static, single object-valued `error` output, non-retryable
classification, and failure-branch fallback. A graph may select that exact
output through an ordinary DAG edge. Revision semantic validation remains the
authority for the exact descriptor, failure contract, and selected handle.

The compiler emits `cloud.workflow.plan.v9` when at least one ordinary
Workflow-local Output selects that descriptor-bound edge. The resulting
immutable WorkflowRun uses input/runtime/Flow version 17 and runtime build
`a3s-cloud-workflows@19`; builds `@1` through `@18` remain explicit replay
entries. Plans v1-v8 and WorkflowRun inputs v1-v16 retain their exact bytes and
behavior. Plan v9 is cumulative and may preserve independently admitted
Execution, Connector, Application, and Transform failure routes.

Runtime v17 schedules the routed Output once with Flow's existing
continue-on-failure action and no retry. On replay, it derives a bounded
`cloud.workflow.step-failure.v6` value with classification
`workflow_local_invalid`, the fixed message `Workflow Output evaluation was
invalid`, no details, and the exact step identity. It selects the descriptor's
`error` handle and lets ordinary DAG reachability activate the failure sink.
Raw evaluator diagnostics never become DAG data or public projection errors.

Projection reconstructs the same typed result from immutable WorkflowRun input
and verified Flow history. The source Output remains `failed`, exposes the exact
selected handle and fixed redacted error, and has no successful result. Its
failure sink may still complete the parent WorkflowRun.

Migration `143` already admits failed Output selected-handle evidence and
rejects completed Output aliases. Aggregate validation still proves Plan v9,
the exact descriptor failure contract, declared edge, failure-v6 shape, and
selected handle before persistence. No public REST shape changes because the
existing plan and typed projection fields already carry these values; the
maintained TypeScript client now enumerates Plan v5-v9 and failure v2-v6.

## Consequences

- Deterministic Output evaluation failures use the same descriptor, Plan, DAG,
  Flow, and projection mechanisms as other admitted step failures.
- A routed local failure executes exactly once and cannot leak raw evaluator
  diagnostics through handled data or the public step projection.
- Historical Plans, Run inputs, and runtime builds keep exact replay behavior.
- This adds no table, column, queue, worker, scheduler, timer, retry engine,
  provider path, public route, or second orchestration authority.
