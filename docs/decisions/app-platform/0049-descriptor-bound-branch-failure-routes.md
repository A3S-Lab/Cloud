# 0049: Route Workflow-local Branch failures through descriptor-bound DAG edges

Status: Accepted

## Context

Workflow-local Branch steps evaluate one immutable selector and map its scalar
value to an ordinary configured If / Else handle. A missing selector value or an
invalid projected input previously failed the entire WorkflowRun, even when the
exact Branch descriptor and graph declared an `error` output.

Branch steps differ from other local steps because their successful edges also
carry source handles. Treating every handled edge as a failure route would merge
business routing with failure authority. Copying selector diagnostics into graph
data could disclose input values, and retrying the same deterministic invalid
evaluation cannot change its result. Reinterpreting Plans 1 through 9 or
WorkflowRun versions 1 through 17 would break immutable replay behavior.

## Decision

An admitted Workflow-owned Branch descriptor with semantic profile
`workflow.if-else` may declare one required, static, single object-valued
`error` output, non-retryable classification, and failure-branch fallback. A
graph may select that exact output through an ordinary DAG edge. Configured
routes and the default remain business handles, must exactly match the remaining
outgoing handles, and cannot alias the descriptor error handle.

The compiler emits `cloud.workflow.plan.v10` when at least one Branch selects
that descriptor-bound edge. The resulting immutable WorkflowRun uses
input/runtime/Flow version 18 and runtime build `a3s-cloud-workflows@20`; builds
`@1` through `@19` remain explicit replay entries. Plans v1-v9 and WorkflowRun
inputs v1-v17 retain their exact bytes and behavior. Plan v10 is cumulative and
may preserve independently admitted Execution, Connector, Application,
Transform, and Output failure routes.

Runtime v18 schedules the routed Branch once with Flow's existing
continue-on-failure action and no retry. On replay, it derives a bounded
`cloud.workflow.step-failure.v7` value with classification
`workflow_local_invalid`, the fixed message `Workflow Branch evaluation was
invalid`, no details, and the exact step identity. It selects only the
descriptor's `error` handle and lets ordinary DAG reachability activate the
failure sink. Raw selector diagnostics never become DAG data or public
projection errors.

Projection reconstructs the same typed result from immutable WorkflowRun input
and verified Flow history. The source Branch remains `failed`, exposes the exact
selected error handle and fixed redacted error, and has no successful result.
Its failure sink may still complete the parent WorkflowRun. The existing Branch
projection already permits an exact selected handle on a failed source, so no
database migration is required.

The public REST shape is unchanged because the existing plan and typed
projection fields carry these values. The maintained TypeScript client now
enumerates Plan v10, compiler v10, and failure v7.

## Consequences

- Deterministic Branch evaluation failures use the same descriptor, Plan, DAG,
  Flow, and projection mechanisms as other admitted step failures.
- Business If / Else handles remain distinct from descriptor failure authority.
- A routed local failure executes exactly once and cannot leak raw evaluator
  diagnostics through handled data or the public step projection.
- Historical Plans, Run inputs, and runtime builds keep exact replay behavior.
- This adds no table, column, queue, worker, scheduler, timer, retry engine,
  provider path, public route, or second orchestration authority.
