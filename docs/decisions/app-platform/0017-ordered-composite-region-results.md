# 0017: Reconstruct composite regions by stable ordinal

Status: Accepted

## Context

Decision 0016 defines one exact frame and frame result for each Iteration or
Loop child. Durable children may finish in a different order from their
zero-based semantic positions. A parent cannot expose observation order as
business output or apply variable writes in that order without making replay
and recovery nondeterministic.

Workflow owns this product-level reduction. A3S Flow continues to own durable
scheduling, waits, replay, cancellation, history, and child linkage.

## Decision

Workflow defines a bounded
`cloud.workflow.composite-region-result.v1` runtime value. It binds the same
organization, project, parent run, PlanRevision, Plan, variable-contract, and
composite-policy authority as every included frame. Observed frame resolutions
are sorted into unique contiguous ordinals beginning at zero before any output
or variable reduction occurs.

Iteration emits an ordinal array. `terminate` rejects the region on a failed
frame, `continue_null` retains that ordinal as JSON null, and `remove_failed`
omits the failed value while retaining the failed frame evidence. An empty
Iteration emits an empty array. Loop requires at least one successful frame,
requires its configured termination path to resolve to a boolean for every
output, rejects frames after the first true value, and exposes the terminal
child output.

Successful frame Run updates and explicit exports are folded in ordinal order;
later ordinals deterministically replace earlier values for the same declared
target. The self-contained result retains every exact frame and resolution,
the bounded business output, reduced maps, and canonical digests. Invalid
failure text, ordinal gaps, duplicate ordinals, authority drift, premature or
late Loop termination, output overflow, and digest drift fail closed.

## Consequences

Completion observation order can no longer change Iteration output, Loop
termination, Run updates, or explicit exports. The reducer is pure runtime
JSON authorized by existing ACL contracts. It adds no variable or region
table, scheduler, queue, worker, event log, child lifecycle, or Flow command.

This reducer decision alone did not execute Iteration or Loop and remained
outside runtime build `a3s-cloud-workflows@2`. Decision
[0018](0018-authority-bound-composite-child-workflow-runs.md) subsequently
registers Flow-backed dispatch in build `a3s-cloud-workflows@3` and supplies
durable child creation, linkage, cancellation, and recovery without adding a
Cloud-local orchestrator.
