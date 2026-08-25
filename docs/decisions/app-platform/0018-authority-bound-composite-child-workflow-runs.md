# 0018: Coordinate composite frames through authority-bound WorkflowRuns

Status: Accepted

## Context

Decisions 0016 and 0017 define deterministic composite frames and ordinal
region reduction. Executing a frame also needs one durable child lifecycle.
A3S Flow's native child-workflow command assigns its own child run identity,
while Cloud's ordinary WorkflowRun must bind a predetermined WorkflowRun,
Operation, Goal, PlanRevision, Outbox request, and tenant authority. Using both
identities for one child would split replay from the product projection.

Workflow must therefore dispatch the exact child without introducing another
scheduler, queue, run table, or workflow engine. Parent cancellation and
timeout must also wait for every admitted child to reach a terminal state.

## Decision

Composite-enabled runs use immutable WorkflowRun input/runtime/Flow version 3
and replay build `a3s-cloud-workflows@3`. Versions 1 and 2 remain registered
for historical and non-composite runs.

For each zero-based frame, the parent Flow creates one authority-bound hook
named `workflow-composite:<step-id>:<ordinal>`. Its metadata contains the exact
digest-bound frame. The Workflow application port derives the child
WorkflowRun ID with UUID v5 from the parent run ID and frame digest, then
derives the Goal and PlanRevision IDs from that child ID. It creates or adopts
the ordinary immutable Goal, Plan, WorkflowRun, Operation, and Outbox records
through their existing repositories and compiler paths.

The coordinator validates the hook creation event, token, metadata, Plan,
variable contract, composite policy, and frame digest before dispatch. It
validates the correlated child Flow name, version, input, runtime identity,
and history before adding an A3S Flow `ChildOperationReference` of kind
`workflow_run`. The reference binds both parent and child authorities in
`cloud.workflow.composite-child-reference.v1` metadata.

When the child becomes terminal, the coordinator reduces a successful output
through the exact frame or records a bounded failed-frame resolution. It
resumes the parent hook only with a digest-bound
`cloud.workflow.composite-resume.v1` payload. Invalid or altered payloads are
non-deterministic replay failures. Valid child failure follows the immutable
Iteration failure mode or fails a Loop normally.

Iteration frames in runtime v3 are dispatched one at a time in ordinal order;
runtime v22's bounded parallel behavior is defined separately by decision
[0053](0053-bounded-parallel-iteration-waves.md) without reinterpreting this
history. Loop passes the previous child output and ordered Run updates to
the next frame, enforces its maximum-iteration and time budgets, and stops only
when the declared boolean termination path is true. Parent cancellation or
timeout adopts every linked child, requests ordinary WorkflowRun cancellation,
and waits for terminal child projection before terminating the parent Flow.

## Consequences

Iteration and Loop now execute through the same WorkflowRun, Operation,
Outbox, A3S Flow, projection, cancellation, and recovery mechanisms as any
other Workflow run. Coordinator restart adopts the deterministic child rather
than creating a duplicate. Exact child references and resume payloads make
identity or history drift fail closed.

This change adds no region table, mutable variable store, scheduler, queue,
worker, event history, or second orchestration mechanism. Decision 0053 later
retains these same ordinal, authority, cancellation, and replay guarantees for
bounded parallel Iteration waves. Applications-owned variables, Answer/error
branches, compensation, and remaining provider steps are separate gates.
