# 0021: Compose Connector responses through immutable objects

Status: Accepted

## Context

Connector WorkflowRun version 5 deliberately retained only response digests and
byte counts. That made provider attempt, observation, wait, retry, and replay
semantics durable without placing provider response bodies in A3S Flow or
PostgreSQL. The HTTP Request foundation also needs an exact response value that
later typed consumers can resolve without making Flow a response-data store.

C6 is already the sole authority for one fenced provider attempt and its
terminal evidence. The shared immutable-object client is already the sole
object-storage authority. A response contract must preserve both boundaries,
keep ordinary digest-only Connector consumers unchanged, and fail closed across
the provider-call, object-write, and terminal-settlement crash windows.

## Decision

Connectors owns `cloud.connector.response-object.v1` and one typed adapter over
the shared immutable-object client's `connector-responses` child namespace. An
accepted response is keyed by exact organization, project, environment,
Connector profile, immutable revision, attempt UUID, SHA-256 digest, and byte
count. Its relative reference is derived as
`attempts/<attempt>/sha256/<digest>/body`; callers cannot choose another path.
Every write and read verifies the exact digest and bounded length.

The object write happens after the one provider call and before C6 commits
accepted terminal evidence. Only after the idempotent immutable write succeeds
may the existing atomic attempt/evidence settlement become terminal. If the
object write fails, the attempt remains `dispatching` and eventually becomes
indeterminate; neither Workflow nor Connectors may call the provider again. If
settlement fails after the object write, the in-process settlement command
retains the verified reference. Process death can leave an unreferenced object,
but an object without terminal C6 evidence grants no execution authority.

WorkflowRun input/runtime/Flow version 6 requests this mode and records only
`cloud.workflow.connector-response-object.v1`, the attempt ID, opaque relative
reference, digest, and length in version-2 hook evidence, resume payloads, and
step results. New Operations pin replay build `a3s-cloud-workflows@6`. Historic
WorkflowRun version 5 and runtime build `@5` remain registered and reproduce
their original body-free bytes; builds `@1` through `@4` remain compatible as
before. Digest-only Connector callers continue using the existing execution
method and do not write response objects.

## Consequences

Provider response bytes have one storage authority and never enter Flow events,
Workflow tables, Connector evidence rows, logs, or API responses. Replay of a
version-6 accepted attempt verifies that the referenced object still exists and
matches its digest and length before returning the reference. Missing,
conflicting, or corrupt content fails closed.

This change adds no table, migration, queue, worker, scheduler, retry counter,
child Operation, provider client, credential authority, or configuration
language. It completes the immutable Connector response-object composition
slice of W0.4, but it does not make the HTTP Request node or AUT0.5 available;
remaining provider/consumer wiring, response consumption, revocation and
recovery operations, retained integration evidence, and public-interface gates
still apply.
