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

Every newly created Operation pins runtime build `a3s-cloud-workflows@15` and
workflow patch marker `cloud.flow.bounded-step-retries-v1`. One shared Cloud
adapter maps that marker to eight total attempts, clamps the configured initial
delay to 30 seconds, caps exponential progression at 30 seconds, and asks Flow
to replay the workflow after exhaustion. Each owning workflow already observes
the durable failed step and selects its explicit failure, cleanup, or
compensation path.

An unmarked history receives the exact former
`RetryPolicy::fixed(u32::MAX, configured_delay)` value with the default
fail-run exhaustion action. Runtime generations `@1` through `@14` remain
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

Runtime build `@15` also makes object-namespace recovery v2 the current
contract. Seal, restore, verification, and cleanup are deterministic Flow
pages capped at 32 objects or 64 MiB, with 4,096 checkpoints as the hard upper
bound. Delete freezes its exact recovery cleanup plan before mutation and
removes the latest manifest replay anchor only after retained-restore
verification. Workflow v1 remains a distinct exact one-step replay path.

A PostgreSQL 17 CI gate terminates the worker before `StepCompleted` at the
second seal, restore, and recovery-cleanup pages, then reconstructs each run
with a fresh runtime and event store. This closes the object-namespace portion
of the durable-activity convergence item without introducing another
checkpoint repository or lifecycle authority.
