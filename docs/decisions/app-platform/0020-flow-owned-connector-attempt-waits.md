# 0020: Keep Connector attempt and wait decisions in Flow

Status: Accepted

## Context

The Connectors-owned C6 service is the sole authority that reserves, fences,
dispatches, and records evidence for one exact provider attempt. C8 binds that
attempt to an immutable WorkflowRun, Plan, step attempt, Connector revision,
and request digest. C9 freezes the bounded provider-attempt budget and fallback
delay in the existing Workflow policy payload.

Executing a Connector step still requires deterministic observation, waiting,
and retry decisions. Putting those decisions in Connectors would create a
second scheduler beside A3S Flow. Treating an indeterminate C6 observation as
retry permission could duplicate an external side effect. Persisting provider
bodies in Flow history would also create another response-data authority before
the W0.4 immutable response-object contract exists.

## Decision

Connector-enabled runs use immutable WorkflowRun input/runtime/Flow version 5
and replay build `a3s-cloud-workflows@5`. Versions 1 through 4 and runtime
builds `@1` through `@4` remain explicitly registered for replay.

For each provider attempt and observation, Flow creates one deterministic hook
named `workflow-connector:<step-id>:<attempt>:<observation>`. Its metadata binds
the exact organization, project, environment, WorkflowRun, Plan revision and
digest, step, Connector profile and revision, capability, canonical effective
input and digest, policy and digest, provider-attempt number, and observation
number. The coordinator verifies the matching hook creation history before it
calls the Connectors-owned Workflow port.

C6 remains the only provider dispatch, attempt-fencing, and terminal-evidence
authority. Flow interprets its body-free result as follows:

- accepted evidence completes the semantic step with a body-free digest and
  byte-count result;
- rejected evidence fails the step;
- retryable evidence schedules one durable Flow wait, using bounded provider
  `Retry-After` when present and otherwise the immutable C9 fallback delay,
  then opens the next deterministic provider attempt;
- a deferred observation schedules one durable Flow wait and observes the same
  provider attempt again without consuming the retry budget; and
- an indeterminate observation fails closed and never authorizes a blind
  provider retry.

Every successor observation or attempt requires the exact preceding received
hook and completed Flow wait. Attempt and observation counts are bounded, wait
deadlines cannot exceed the parent deadline, and payload or history drift is a
non-deterministic replay failure. Projection reconstructs Connector state and
accepted output from immutable input plus the sole Flow history.

## Consequences

Workflow now owns deterministic Connector scheduling, observation, retry, and
wait interpretation while Connectors continues to own every external attempt.
Coordinator restart or redelivery adopts the same hook and C6 evidence rather
than dispatching through another path.

This change adds no table, queue, timer worker, scheduler, retry counter, child
Operation, response-body store, credential authority, provider configuration,
or HTTP client. The HTTP Request node remains unavailable until W0.4 defines
and composes its immutable response object and the remaining provider,
revocation/recovery, retained integration, and public-interface gates pass.
