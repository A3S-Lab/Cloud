# 0044: Pin bounded infrastructure step retries to new Flow histories

Status: Accepted

## Context

Agent, Build, Data recovery, Deployment, and Execution workflows previously
scheduled infrastructure steps with `RetryPolicy::fixed(u32::MAX, delay)`.
That kept transient failures durable, but a permanent dependency failure could
leave an Operation suspended indefinitely. Constant delays also synchronized
workers after a shared outage.

Changing the retry command emitted while replaying an existing history would
violate Flow determinism. The persisted workflow specification therefore must
decide which retry contract applies; process configuration or deployment time
cannot reinterpret an in-flight run.

## Decision

Cloud pins A3S Flow `1.0.0` revision `7c76eda9`, whose additive
`RetryPolicy::exponential` contract supplies a finite attempt budget, a maximum
delay, and deterministic full jitter derived from immutable run, step, and
failed-attempt identities. Its fixed policy retains the previous serialized
shape.

Every newly created Operation pins runtime build `a3s-cloud-workflows@14` and
workflow patch marker `cloud.flow.bounded-step-retries-v1`. One shared Cloud
adapter maps that marker to eight total attempts, clamps the configured initial
delay to 30 seconds, caps exponential progression at 30 seconds, and asks Flow
to replay the workflow after exhaustion. Each owning workflow already observes
the durable failed step and selects its explicit failure, cleanup, or
compensation path.

An unmarked history receives the exact former
`RetryPolicy::fixed(u32::MAX, configured_delay)` value with the default
fail-run exhaustion action. Runtime generations `@1` through `@13` remain
explicitly replay-compatible, and legacy unpinned histories remain visible
migration debt. The policy branch is based only on the immutable marker.

A3S Flow remains the sole retry clock, suspension, deadline, and attempt
authority. Cloud adds no retry table, counter, sleep loop, random state,
scheduler, queue, or product configuration field.

## Consequences

New Operations cannot retry one infrastructure step forever, and simultaneous
failures spread deterministically below a bounded delay. Final failure remains
visible in Flow history before the product workflow chooses its existing
terminal behavior. Historical `step_created` events replay byte-for-byte.

This decision completes the finite retry portion of the durable-activity
convergence item only. Object namespace recovery can still process a bounded
but namespace-sized payload in one step; deterministic page checkpoints and
process-death evidence remain required before that item is complete.
