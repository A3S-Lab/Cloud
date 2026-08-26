# 0064: Compensate accepted Connector effects during Flow cancellation

Status: Accepted

## Context

A3S Flow 1.1 makes cancellation cleanup-aware. A durable cancellation request
deactivates work that existed before the request, replays the workflow with the
request in history, permits stable cleanup work, and reaches `Cancelled` only
when workflow code returns the terminal cancellation command.

Cloud already binds each Connector Service step to an immutable revision,
bounded retry policy, stable attempt identity, terminal evidence, and a typed
response-object projection. Immediate cancellation could preempt typed-response
materialization and discard the accepted response before it became exact
compensation input. Creating a second cancellation engine, provider retry rail,
or mutable compensation log would duplicate Flow and Connector authority.

## Decision

Static Workflow policy v4 may bind one Connector source step to one exact
Connector compensation step. Revision and Run admission require both steps to
be `connector.http` Services, require the source output schema to equal the
target input schema, require a downstream graph path, and require the target to
have one explicit handled ordinary route. A target is unique and cannot itself
own cancellation compensation.

WorkflowRun input/runtime/Flow v23 is selected exactly when this policy exists.
After Flow records cancellation, Cloud examines declared sources in reverse
plan order. Only a source with accepted terminal evidence is eligible. When
cancellation preempts its ordinary typed-response materializer, v23 schedules
the same immutable response-object read under a distinct stable post-cancellation
Flow step identity. The validated typed output becomes the target input. A
purpose-bound Connector hook v4 and Workflow-to-Connector attempt purpose give
the compensation invocation stable Flow and C6 identities distinct from
ordinary execution while reusing the existing retry, immutable response-object,
typed projection, and coordinator dispatch paths.

If the target's ordinary invocation already produced an accepted effect, Cloud
does not invoke it again. Disposed or otherwise indeterminate source or target
effects fail closed. Once every eligible compensation is durably complete,
the workflow returns Flow's terminal cancellation command. Replay and repeated
cancellation create no second hook, provider attempt, cleanup response
materializer, or terminal event.

Migration `158` only adds `cloud.workflow.policy.v4` to the existing closed
Workflow payload schema registry. It adds no table, column, queue, scheduler,
provider cancellation API, HTTP client, object client, retry counter, or second
history.

Runtime build `a3s-cloud-workflows@25` adds Flow version 23 and explicitly
retains builds `@1` through `@24` for replay.

## Consequences

Cloud has component-level cancellation compensation for accepted exact
Connector effects while A3S Flow remains the sole cancellation and replay
authority. The distinct cleanup response identity closes the race between an
accepted external effect and cancellation of its ordinary typed-response step.
The contract composes with existing parallel, variable, local-step, Application,
and composite runtime semantics through v23.

This decision does not certify provider-side cancellation or revocation,
arbitrary compensation code, compensation for indeterminate effects, retained
PostgreSQL or real-provider recovery, public HTTP Request availability, or
completion of `AUT0.5`, `W0.4`, or `W0.5`.
