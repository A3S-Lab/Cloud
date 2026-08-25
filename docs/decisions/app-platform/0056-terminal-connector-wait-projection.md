# 0056: Project terminated Connector waits from the terminal Flow event

Status: Accepted

## Context

A deferred Connector observation is already bound to its exact provider
attempt. Its Flow Hook is `received`, while an ordinary durable wait and the
typed-response step remain open. A parent cancellation makes that open work
non-actionable; an immutable deadline terminates the run and removes it from
scheduled wakeups.

The prior Cloud projection continued to use the older typed-response step event
for this state. It therefore tried to change the Connector projection from
running to cancelled or failed without advancing its Flow sequence, and the
monotonic projection guard correctly classified the update as replay drift.

## Decision

When a Flow run is terminal, a Connector has neither a completed typed result
nor a Connector-classified failure, and its projection changes because of that
run transition, Cloud uses the terminal Flow event sequence and timestamp. A
completed Connector result and a Connector-classified failure continue to use
their own exact response-step or Hook evidence sequence.

For parent cancellation, the coordinator does not invoke the Connector port
again. Flow cancels the durable wait, the WorkflowRun becomes `cancelled`, the
Service projection becomes `cancelled`, and the exact existing Connector
attempt URN remains attached. For immutable deadline expiry, the Flow timeout
event produces a `timed_out` WorkflowRun and failed Service projection; the
terminal run exposes no due wakeup. A replacement coordinator observes either
terminal state without a second provider call or terminal event.

This change reuses the existing WorkflowRun input/runtime/Flow v8, Connector
attempt, Flow cancellation, timeout, wait, and projection authorities. It adds
no schema version, table, queue, scheduler, retry counter, provider cancellation
API, HTTP client, or object client.

## Consequences

Cloud has component evidence that one deferred Connector attempt is fenced
across parent cancellation, immutable deadline expiry, and coordinator
replacement without blind redispatch. This does not cancel an already-started
provider-side effect, revoke a Connector revision or Secret, run compensation
because of cancellation, certify multi-day PostgreSQL/provider recovery, make
HTTP Request public, or complete `AUT0.5`, `W0.4`, or `W0.5`.
