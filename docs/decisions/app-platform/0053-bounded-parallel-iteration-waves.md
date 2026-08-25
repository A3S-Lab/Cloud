# 0053: Execute bounded Iteration frames in authority-bound waves

Status: Accepted

## Context

The immutable Iteration policy already freezes `maximum_concurrency` between
one and ten, but WorkflowRun generations v3 through v21 intentionally execute
one composite frame at a time. Treating the field only as a validation bound
does not provide the admitted parallel behavior.

A3S Flow exposes one durable Hook command at a replay point. Adding a Cloud
scheduler, queue, worker, mutable batch record, or alternate child lifecycle
would duplicate Flow and WorkflowRun authority. Starting children without
stable per-frame identities would also make process-death adoption, evidence
correlation, cancellation, and replay ambiguous.

Historic WorkflowRun inputs cannot acquire new parallel side effects. A new
runtime generation is therefore required even though the public REST shape and
the composite policy ACL remain unchanged.

## Decision

WorkflowRun input/runtime/Flow v22 is selected for every newly compiled graph
that contains an Iteration policy with `maximum_concurrency > 1`. Inputs v3
through v21 retain their existing serial replay behavior, including historic
policies whose bound is greater than one. Runtime v22 composes the previously
admitted Plan v2-v11, Application, Connector, failure-route, Variable
Aggregator, and List Operator semantics.

Runtime v22 partitions the immutable Iteration input into contiguous waves of
at most `maximum_concurrency` frames. It creates one Flow Hook per wave. The
Hook metadata binds the tenant, parent WorkflowRun, exact Plan revision and
digest, region step, first ordinal, ordered effective inputs, shared available
variables, and a canonical digest. Exact child frames are reconstructed from
that material and the pinned variable/default/composite contracts. Each frame
keeps the same deterministic child Goal, Plan, WorkflowRun, Operation, and
child-reference identities used by serial execution.

The existing Workflow composite execution port starts or adopts every frame in
the active wave concurrently. Each ordinary child WorkflowRun is verified
against its own Flow history and durably linked to the parent before the wave
can resume. A replacement coordinator repeats the same requests and adopts the
same children after process death. Transient dispatch or linking failures leave
the Flow Hook active for retry; deterministic admission rejection becomes a
bounded failed frame without inventing a child reference.

The wave resumes only after every admitted child is terminal and every created
child is linked. `continue_null` and `remove_failed` wait for the complete wave
and reduce results by stable ordinal. `terminate` cancels every non-terminal
sibling and waits for their terminal observations before resuming the parent
failure. The wave receipt distinguishes that primary failure from consequent
sibling cancellations so ordinal sorting cannot mask the original error.
Parent cancellation and timeout likewise adopt, cancel, and await all children
in every observed wave before terminating the parent.

Runtime build `a3s-cloud-workflows@24` adds Flow version 22 and explicitly
retains builds `@1` through `@23` for replay. The Cloud dependency remains
exactly pinned to A3S Flow 1.0.0. This change adds no REST/OpenAPI field,
database migration, product configuration format, mutable wave store, worker,
queue, retry engine, or second orchestration authority.

## Consequences

- New parallel Iterations execute at most ten child WorkflowRuns at once.
- Only one wave is active at a replay point; later waves begin after the prior
  wave has durable terminal evidence.
- Output, Run-variable updates, exports, and failure interpretation remain
  deterministic because reduction always uses zero-based ordinal order.
- Child effects remain recoverable through ordinary WorkflowRun, Operation,
  Outbox, and Flow authorities.
- Historic runtime generations retain byte-stable serial behavior.
- No public API or ACL schema version changes are required.
