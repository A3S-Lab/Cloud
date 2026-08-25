# 0055: Compose Connector compensation from ordinary durable steps

Status: Accepted

## Context

A3S Flow 1.0 models compensating work as ordinary durable steps selected from
recorded workflow state; it does not define a separate compensation command or
runtime. The Cloud Connector path already binds each Service step to one exact
immutable Connector revision, one stable provider-attempt identity, bounded
Flow observation, and an authorized immutable response object.

A recoverable business outcome therefore must remain an accepted, typed
Connector result that an ordinary Branch can inspect. Infrastructure failures,
indeterminate observations, and exhausted provider attempts remain on the
existing retry or descriptor-bound failure paths and cannot be reclassified as
a compensable business outcome.

## Decision

The component-only Connector compensation contract is the exact ordinary DAG
sequence `reserve -> charge -> branch -> release`. A successful charge follows
the completion edge. A typed accepted charge result with `ok = false` follows
the compensation edge, passes the original domain result to the exact
`release` Connector step, and completes only after that step has produced its
terminal typed result.

Each Connector step retains its own immutable profile/revision/digest binding
and stable Flow-derived attempt identity. The completed aggregate retains both
the original charge failure value and the release result. Exact terminal hook
redelivery is a no-op: it appends no history, rereads no response object, and
creates no second release attempt.

This composition reuses the existing Plan v2, WorkflowRun input/runtime/Flow
v8, Connector policy, hook, C6 attempt, and C10/C11 response-object authorities.
It adds no compensation engine, table, queue, scheduler, retry counter, HTTP
client, object client, public endpoint, or schema version.

## Consequences

Cloud now has retained component evidence for one exact Connector domain-result
compensation composition aligned with A3S Flow 1.0. It does not certify general
multi-step reverse-order compensation, cancellation-triggered compensation,
provider event-consumer recovery or revocation behavior, retained PostgreSQL or
real-provider evidence, public HTTP Request availability, or completion of
`AUT0.5`, `W0.4`, or `W0.5`.
