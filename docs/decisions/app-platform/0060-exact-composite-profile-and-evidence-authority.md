# 0060: Bind composite profiles and evidence to exact runtime authority

Status: Accepted

## Context

Iteration and Loop intentionally share the existing coarse `subworkflow` step
kind and the same exact child `workflow.run` capability. Their execution
semantics are distinguished by the revision-owned descriptor and immutable
composite-region policy. Checking only the coarse kind, region coverage, and
child capability would allow a Plan to bind a Loop policy to an Iteration
descriptor, or the reverse, while still passing runtime admission.

Each admitted child is also linked to the parent through an A3S Flow
`ChildOperationLinked` event. That event changes the Subworkflow projection's
bounded evidence references independently of the Hook creation or receipt
event. Reusing only the Hook sequence for the changed evidence would make two
different projections claim the same Flow sequence and correctly trigger the
domain replay-drift fence during a later Loop iteration or coordinator restart.

## Decision

Workflow composite runtime admission requires every covered Subworkflow Plan
step to bind the exact semantic profile declared by its immutable region
policy. An Iteration region accepts only `workflow.iteration`; a Loop region
accepts only `workflow.loop`. Missing descriptors, profile drift, incomplete
region coverage, and non-exact child Workflow revisions fail before execution.

The Subworkflow projection derives its causal sequence from the greater of the
current region Hook event and the latest `ChildOperationLinked` event for a
linked frame in that exact region. Evidence references remain reconstructed
from verified Flow history and retain their existing bounded canonical order.
Unrelated parent events do not advance the step projection.

Replacement coordinators repeat the same deterministic frame request, adopt
the same child WorkflowRun, and may add the next frame only after the prior
terminal output has resumed the parent Hook. Sequential Loop output, maximum
iterations, and the immutable region time budget remain owned by the existing
WorkflowRun runtime over A3S Flow 1.1.0.

This change adds no configuration shape, database migration, mutable region or
evidence store, scheduler, queue, worker, retry rail, public route, or second
orchestration authority. It does not make Iteration or Loop publicly available.

## Consequences

- Coarse `subworkflow` identity can no longer weaken exact Iteration or Loop
  semantics at runtime.
- Linking a later child frame advances evidence projection with the exact Flow
  fact that changed it, preserving replay-drift detection.
- Sequential Loop execution remains restart-safe and deterministically bounded.
- Historical immutable input and Flow histories retain their existing replay
  shape.
