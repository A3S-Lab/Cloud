# 0024: Project Connector JSON through one typed Flow step

Status: Accepted

## Context

Decision 0021 gives an accepted Connector response one immutable object and
keeps only its exact reference in WorkflowRun version 6. Decision 0022 permits
an in-process consumer to resolve that object only after environment
authorization, accepted terminal C6 evidence, and another integrity check.
Version 6 intentionally stops at the reference, so no ordinary typed Workflow
node output exists for the HTTP Request component. Decision 0023 separately
assigns version 7 to exact finite-Execution default-output interpretation.

A consumer must turn authorized bytes into a deterministic value without
giving Flow direct object-store authority, persisting raw response bytes in
history, copying Connector evidence, or confusing response interpretation with
provider retry permission. Historical version-5 digest-only, version-6
reference-only, and version-7 default-output runs must retain their exact
behavior.

## Decision

New Connector-enabled Plan v2, Plan v3, and Plan v4 runs use immutable
WorkflowRun input/runtime/Flow version 8 and replay build
`a3s-cloud-workflows@8`. Runtime builds `@1` through `@7` remain explicit
replay-compatible entries. Version-8 Connector hooks use
`cloud.workflow.connector-hook.v3`; accepted hook evidence still contains only
the exact attempt-scoped immutable reference, digest, and byte count. Plan v4
without a Connector remains version 7, while Plan v4 with a Connector composes
its existing default-output authority into version 8.

After verifying accepted hook authority, Flow creates one dedicated
`workflow_connector_response` step. Its creation event must contain the exact
serialized Workflow step, Connector metadata, terminal evidence, step name,
and `RetryPolicy::none()`. Projection rejects a missing terminal step,
additional creation event, changed input, changed step name, or retry-policy
drift.

The step calls only `IConnectorResponseObjectPort`, carrying the exact
environment-scoped authorization and derived response reference. It accepts
exactly one JSON value, rejects duplicate object keys and trailing content,
validates the immutable Workflow output schema, enforces the existing bounded
Workflow output size, and computes the ordinary typed result digest. Only that
schema-validated typed value is recorded as the Flow step and Workflow node
output. Raw object bytes, direct object credentials, and a public response-body
read are not recorded or exposed.

Any read, integrity, JSON, duplicate-key, schema, size, or history failure
fails the response step closed. The no-retry policy prevents deterministic bad
content from causing repeated reads and never authorizes another provider
attempt. A completed replay reuses the durable typed step result. Version 7
retains exact default-output semantics, version 6 continues to return only the
immutable reference, and version 5 continues to return only digest and
byte-count evidence.

## Consequences

The component HTTP Request execution path now produces a bounded typed JSON
node value through the existing Connector, Resource Grant, immutable-object,
C6, Workflow, and Flow authorities. Raw provider response representation
remains transient, while the semantically admitted node output participates in
ordinary Flow replay and Workflow projection.

This adds no table, migration, object namespace, direct object client, public
download route, queue, worker, scheduler, retry counter, child Operation,
credential authority, provider call, or configuration language. It does not
make HTTP Request or `AUT0.5` publicly available; remaining provider and Event
consumer wiring, revocation/recovery operations, retained PostgreSQL and
end-to-end evidence, interfaces, and the other W0.4 capability steps still
apply.
