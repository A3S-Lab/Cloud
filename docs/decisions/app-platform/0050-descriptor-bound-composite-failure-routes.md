# 0050: Route composite-region failures through descriptor-bound DAG edges

Status: Accepted

## Context

Workflow-owned Iteration and Loop steps execute one immutable composite-region
policy through ordinary child WorkflowRuns. A validated child failure, item
bound, Loop time budget, maximum iteration exhaustion, or local composite
finalization failure previously failed the parent WorkflowRun even when the
exact descriptor and graph declared an `error` output.

Those failures differ from resume-authority drift. The former are deterministic
outcomes of pinned child evidence or policy; the latter means replay no longer
matches immutable frame authority and must remain non-deterministic. Copying raw
child or reducer diagnostics into graph data could disclose provider or child
details. Reinterpreting Plans 1 through 10 or WorkflowRun versions 1 through 18
would break immutable replay behavior.

## Decision

An admitted Workflow-owned `workflow.iteration` or `workflow.loop` descriptor
with one bound composite region may declare one required, static, single
object-valued `error` output, non-retryable classification, and failure-branch
fallback. A graph may select that exact output through an ordinary DAG edge.
Revision semantic validation remains the authority for the descriptor, region
binding, failure contract, and selected handle.

The compiler emits `cloud.workflow.plan.v11` when at least one admitted
composite step selects that descriptor-bound edge. The resulting immutable
WorkflowRun uses input/runtime/Flow version 19 and runtime build
`a3s-cloud-workflows@21`; builds `@1` through `@20` remain explicit replay
entries. Plans v1-v10 and WorkflowRun inputs v1-v18 retain their exact bytes and
behavior. Plan v11 is cumulative and may preserve independently admitted
Execution, Connector, Application, and Workflow-local failure routes.

Runtime v19 keeps the existing authority-bound child hooks and schedules the
composite completion step with Flow's continue-on-failure action only when the
descriptor selects the error port. A validated child failure or deterministic
region-policy failure schedules one ordinary no-retry local materializer. That
step durably records a bounded `cloud.workflow.step-failure.v8` value with
classification `workflow_local_invalid`, the fixed message `Workflow composite
region did not complete`, no details, and the exact step identity. It selects
only the descriptor's `error` handle and lets ordinary DAG reachability activate
the failure sink. Resume-authority drift remains `NonDeterministic` and never
becomes handled graph data.

Projection reconstructs the same typed result from immutable WorkflowRun input
and verified Flow history. Child-hook failures remain correlated to their exact
hook sequence after the materializer completes. The source Subworkflow remains
`failed`, exposes the exact selected error handle and fixed redacted error, and
has no successful result. Its failure sink may still complete the parent
WorkflowRun.

Constraint-only migration `148` admits a selected handle only on a failed
Subworkflow projection. Aggregate validation still proves Plan v11, the exact
descriptor failure contract, bound composite region, failure-v8 shape, and
selected handle before persistence. The public REST shape is unchanged because
the existing plan and typed projection fields carry these values. The
maintained TypeScript client enumerates Plan v11, compiler v11, and failure v8.

## Consequences

- Deterministic composite failures use the existing descriptor, Plan, DAG,
  Flow, and projection mechanisms.
- Resume-authority drift remains a non-deterministic replay fence and cannot be
  downgraded into handled data.
- A routed composite failure is durably materialized once and cannot leak raw
  child or reducer diagnostics through graph data or public projections.
- Historical Plans, Run inputs, and runtime builds keep exact replay behavior.
- This adds no table, column, queue, worker, scheduler, timer, retry engine,
  provider path, public route, or second orchestration authority.
