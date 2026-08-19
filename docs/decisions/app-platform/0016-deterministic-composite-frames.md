# 0016: Reduce composite child results through deterministic frames

Status: Accepted

## Context

WorkflowRevision already owns exact typed-variable and composite-region ACL
contracts, and Plan v2 pins their digests plus one exact child
WorkflowRevision for every admitted Iteration or Loop region. The current
WorkflowRun runtime still rejects `subworkflow`, so those contracts had no
executable boundary for constructing child input or reducing child output.

That boundary must remain deterministic without becoming another workflow
engine. Workflow owns graph semantics and variable reduction; A3S Flow owns
durable scheduling, replay, retry, waits, cancellation, history, and child
operation linkage.

## Decision

Workflow defines a bounded `cloud.workflow.composite-frame.v1` runtime value
for one zero-based region ordinal. A frame binds the organization, project,
parent run, PlanRevision and Plan digest, variable and composite-region
digests, region step and mode, exact child Workflow definition/revision/digest,
typed child input, captured parent/local variables, and a canonical digest.
Frame construction validates every pinned authority, the immutable region
bound, exact child capability, optional digest-backed defaults, and typed read
projection. Applications-owned values remain rejected.

The pure `cloud.workflow.composite-frame-result.v1` reducer accepts one bounded
child output. It materializes the region's raw node outputs, resolves all
assignments from one pre-write snapshot, separates deterministic Run updates,
and crosses the composite boundary only through declared exports. Canonical
frame, child-input, child-output, and result digests make replay identity
explicit. Parent-scope materialization skips assignments owned by a
`subworkflow` step so the frame is the single reducer for that child result.

Frames and results are runtime JSON, not product configuration. The immutable
WorkflowRevision contracts that authorize them remain canonical A3S ACL. This
slice adds no table, mutable variable store, scheduler, queue, worker, retry
engine, event history, or child lifecycle.

## Consequences

Composite input, local mutation, Run updates, and exports now have one bounded,
replay-stable component contract with exact Plan and child-revision authority.
Focused tests prove deterministic serialization, zero-based policy bounds,
typed materialization, atomic assignments, explicit exports, digest drift, and
`Send + Sync` behavior.

This decision does not make Iteration or Loop executable. The deployed runtime
continues to reject `subworkflow` until the existing A3S Flow path records and
schedules frames, links exact child operations, reconstructs deterministic
ordering, and applies cancellation/recovery semantics. That wiring must not
introduce a Cloud-local orchestration mechanism and, because it will change
deployed replay code, must introduce a new runtime build identity under
[0015](0015-versioned-flow-runtime-builds.md). The component-only reducer does
not change the current `a3s-cloud-workflows@2` build.
