# A3S Cloud OpenAPI Contract

A3S Cloud publishes its complete REST contract as OpenAPI 3.0.3. The committed
snapshot and the document served by the control plane are the same bytes:

- repository snapshot: [`openapi/v1.json`](../openapi/v1.json);
- public runtime endpoint: `GET /api/v1/openapi.json`; and
- API base path: `/api/v1`.

The contract is generated from the resolved production route table. A snapshot
test rejects drift between routes and the committed document, and the
compatibility checker rejects undocumented or incompatible changes.

Ordinary API success, error, and streaming responses default to
`Cache-Control: no-store`, `Pragma: no-cache`, and
`Referrer-Policy: no-referrer` through the shared response boundary. Routes
with an explicit transport cache policy retain it; in particular, the public
OpenAPI document remains `public, max-age=300`.

The current semantic contract version is `1.74.0`.

Contract `1.74.0` adds the closed Developer Workflows WorkloadProfile API. An
authorized caller can accept one canonical `a3s.cloud.workload-profile.v1` ACL
bound to an accepted BuildPlan, get the logical profile's current immutable
revision, list its first 1 through 100 continuous revisions in ascending order,
or get one exact revision. Responses preserve canonical ACL, digests, exact
BuildPlan and SourceRevision evidence, typed process/resource/Secret-reference/
port/health/schedule intent, actor, and acceptance time. They expose no Secret
material, source bytes, credentials, checkout state, or downstream
BuildRun/Workload/Execution/Route/Operation/scheduler lifecycle. REST, the
maintained client and CLI, and four Management MCP tools dispatch the same
command and Application query service.

Contract `1.73.0` adds the closed logical Agent execution checkpoint and fork
API. Authorized execution readers can list or get immutable checkpoint
projections, read one digest-verified snapshot, and page the execution's own
semantic event records. An `execution:write` caller can capture a checkpoint at
the latest event or one exact inclusive sequence and can fork it into a new
execution with a caller-owned idempotency key. A new capture returns `201`, a
new fork returns `202`, and exact replay returns `200`.

Checkpoint snapshots contain at most 1,000 events and 896 KiB of canonical
JSON. A checkpoint captured on a fork prepends its already verified inherited
trajectory, so later forks remain self-contained without recursively reading
their ancestry. Cloud writes snapshots through the shared immutable-object
client under the `agent-checkpoints` namespace, while PostgreSQL migration `168`
stores only the digest/size/path projection, exact Agent/provider/invocation
bindings, Runtime telemetry correlation, and immutable parent lineage. A fork
never mutates its parent. A new fork revalidates the published Agent artifact
and the exact selected provider profile. Before provider dispatch, Cloud
reloads and verifies the object and materializes one bounded provider-neutral
trajectory prompt. Missing, corrupt, revoked, or drifted evidence fails closed.
This contract does
not claim provider-private or Box suspend/resume support where the selected
provider exposes no such capability.

Contract `1.72.0` adds the closed Developer Workflows BuildPlan API. An
authorized caller can detect bounded deterministic proposals for one exact
immutable SourceRevision, accept one canonical proposal ACL idempotently, list
accepted plans for that exact revision, or get one accepted plan. Responses
preserve canonical proposal/contract ACL, typed recipe and detector evidence,
digests, immutable acceptance facts, and replay state; they never expose source
bytes, credentials, checkout receipts, local paths, or a BuildRun/Workload/Route
lifecycle. REST, the maintained client and CLI, and four Management MCP tools
dispatch the same CQRS/application boundary.

Contract `1.71.0` adds the closed Agent approval-checkpoint API. Authorized
execution readers can list or get checkpoints, while an `execution:write`
caller can submit one `approved` or `denied` decision with a caller-owned
idempotency key and the exact `x-a3s-expected-version`. A new decision returns
`202`; an exact replay returns `200`. Checkpoints expose immutable provider-run,
invocation-profile, Tool, request-digest, authorization, decision, resume-command,
version, and expiry evidence, but never Tool payload or Secret material. Agent
execution responses admit `awaiting_approval`, and the closed semantic event
union admits `approval_resolved`. Denial, one-day expiry, cancellation, provider
restart, retention gaps, and mismatched resume evidence fail closed.

Contract `1.70.0` adds the nullable closed immutable
`HarnessInvocationProfile` to Agent execution responses after dispatch binds
the exact Runtime. It documents exact Agent, provider, instructions and policy
digests, workspace, Skill, MCP, model, Secret-reference, Tool, and required
capability fields without returning mutable configuration or Secret material.
All binding arrays are duplicate-free and sorted by `(assetId,
assetReleaseId)`, `(modelId, modelRevisionId)`, Secret `name`, or Tool `(name,
revision)` as applicable; required capabilities use lexical wire-value order.
Secret environment, file, and registry targets are collision-free. The
profile's canonical JSON encoding is bounded to 256 KiB.
The existing Agent semantic sequence also admits typed `tool_request` and
`tool_result` records containing only the exact Tool binding and payload
digest, byte length, and media type. Larger Tool content remains outside the
event log under the shared immutable-object authority. PostgreSQL correlates
each accepted Tool record to the versioned `a3s.cloud.agent-tool-audit.v1`
shared audit detail in the same provider-receipt transaction; replay creates
neither duplicate semantics nor duplicate audit. `AgentExecutionEvent` is a
closed discriminator union keyed by `kind`, so standard OpenAPI tooling can
validate each event's exact content schema without treating Tool events as
untyped JSON.

Contract `1.69.0` adds the optional closed `providerKind` selector to Agent
execution creation. Cloud resolves that kind through its admitted provider
registry and persists the exact immutable profile before scheduling. Every
existing Agent conversation, execution, mutation, event-page, and Code change
set response now references a closed operation-specific schema. Execution
responses include the selected provider kind, revision, common and native
protocols, profile digest, and capability digest; no canonical ACL, provider
configuration, environment value, or Secret material is returned.

Contract `1.68.0` extends the existing closed Workflow response enums with
Plan schema/compiler v12, `cloud.workflow.step-failure.v9`, and
`agent_dispatch_rejected`, `agent_execution_failed`, and
`agent_execution_cancelled`. These values expose descriptor-bound Agent failure
routing through the existing Plan and step-projection fields. Failure payloads
contain only a closed classification and fixed redacted message. No route,
request property, response property, or authorization boundary is added.

Contract `1.67.0` adds `cloud.workflow.policy.v4` to the closed Workflow policy
schema enumeration and maintained TypeScript client. Version 4 binds one exact
Connector Service source to one downstream exact Connector compensation step
for Flow-owned cancellation cleanup; only an accepted source effect is eligible
at runtime. It adds no route or response property; older policy and WorkflowRun
histories retain their exact bytes and replay behavior.

Contract `1.66.0` adds the bounded unresolved Connector execution-attempt
collection, an exact safe attempt read, an exact resolution read, and one
idempotent resolution write. The mutation accepts only a bounded control-free
operator `reason`; the resulting closed `indeterminate` conclusion can commit
only after the exact dispatch outcome deadline and is atomically paired with
body-free terminal evidence. Attempt responses expose request/evidence digests,
byte counts, closed state, recovery state, and canonical times, but never the
fence token, request or response bodies, endpoint, credentials, or provider
text. Resolution does not authorize provider retry or cancellation and does not
claim acceptance or rejection.

Contract `1.65.0` adds the exact Connector revision-revocation operations and
fully typed success responses for all existing Connector profile, revision,
history, and revocation operations. The idempotent write accepts one closed
bounded `reason`; both responses expose the exact revision number and digest,
actor, and timestamp without Secret material or provider state. Existing
Connector routes retain their response bytes.

Contract `1.64.0` completes operation-specific success documentation for every
Workflow-tagged route. Ontology collection, aggregate, revision, diff, and
mutation operations now expose reusable closed schemas. HumanTask collection,
aggregate, claim, release, and Form-backed submission operations do the same,
including their assignment policy, release reference, interaction request,
output mapping, lifecycle, and nullable state projections. Contract `1.63.0`
already closed Goal, Plan revision, node-catalog, run, cancellation, wait,
output, variable-inspection, diagnostics, and history payloads. Both increments
preserve existing JSON fields and routes while documenting UUIDs, digests,
finite enums, collection bounds, typed evidence, and exact nested resources.
No authorization boundary, request shape, runtime behavior, or response byte
is changed.

Contract `1.62.0` adds the versioned Workflow payload schema
`cloud.workflow.configuration.list-operator.v1`. Existing Workflow
definition create/revise envelopes continue to carry configuration as bounded
A3S ACL text. Definition, revision-summary, revision, and mutation responses
now reference closed reusable schemas; `payloads` is a discriminated union and
its `schema` enum includes the List Operator and Variable Aggregator values.
The new schema freezes bounded typed filter, one-based extraction, ordering,
and limit semantics for the internally available Workflow-local List Operator.
The maintained TypeScript client enumerates the same schema value. No route,
JSON property, authorization boundary, or response envelope is added.

Contract `1.61.0` adds the versioned Workflow payload schema
`cloud.workflow.configuration.variable-aggregate.v1`. Existing Workflow
definition create/revise envelopes continue to carry configuration as bounded
A3S ACL text, and revision responses continue to return the existing
`payloads[].schema` string plus canonical ACL and digest. The new schema freezes
bounded typed candidate groups for the internally available Workflow-local
Variable Aggregator. The maintained TypeScript client enumerates the new schema
value. No route, JSON property, authorization boundary, or response envelope is
added.

Contract `1.60.0` adds the project-authorized
`GET /organizations/{organization_id}/workflow-runs/{workflow_run_id}/diagnostics`
operation. Its bounded `cloud.workflow-run.diagnostics.v1` response compares
persisted Workflow projection sequence with one verified A3S Flow
snapshot/history observation, reports closed step and Flow statistics plus
redaction-safe diagnostics, and returns at most 256 exact evidence references
with explicit truncation. The endpoint returns the standard wrapped success
envelope and documents missing/denied resources as `404` and an unavailable or
concurrently changing Flow observation as `503`.

Workflow Plan v11 and the bounded `cloud.workflow.step-failure.v8` composite
failure value use the existing plan and step-projection response fields. They
add no route, field, or JSON shape, so this internal execution-semantics
revision does not increment the OpenAPI contract version.

WorkflowRun responses already expose the bounded
`steps[].evidenceReferences` array. Current projections populate only these
closed, canonical URN families:

- `urn:a3s:cloud:connectors:attempt:<uuid>`;
- `urn:a3s:cloud:agents:conversation:<uuid>`;
- `urn:a3s:cloud:agents:execution:<uuid>`;
- `urn:a3s:cloud:executions:execution:<uuid>`;
- `urn:a3s:cloud:forms:submission:<uuid>`;
- `urn:a3s:cloud:operations:operation:<uuid>`;
- `urn:a3s:cloud:workflow:human-task:<uuid>`;
- `urn:a3s:cloud:workflow:workflow-decision:<uuid>`; and
- `urn:a3s:cloud:workflow:workflow-run:<uuid>`.

The array is sorted, duplicate-free, limited to 32 entries, and reconstructed
only from verified A3S Flow history. An Execution terminal observation retains
its exact child Execution and Operation identities. A received Connector
observation retains its deterministic attempt identity, including deferred or
indeterminate outcomes; a dispatch rejection without owning-context evidence
retains no reference. A received HumanDecision resume retains the exact
HumanTask and WorkflowDecision identities; interactive submit, approve, and
reject outcomes also retain the accepted FormSubmission identity, while
automatic expiry and cancellation have no synthetic submission reference.
Each linked Subworkflow frame retains its exact child WorkflowRun and Operation
identities. Iteration and Loop steps select the latest 16 linked frames by
ordinal before canonical sorting, keeping the public array within its existing
32-reference bound; complete frame history remains available from the same
authorized Flow-derived run history.

These URNs are correlations, not embedded evidence or an authorization grant.
Reading any referenced owner resource still requires its normal authorization
boundary. Populating this existing field did not itself change a route or JSON
shape; contract `1.60.0` was introduced by the separate diagnostics operation,
`1.61.0` adds Variable Aggregator payload semantics, `1.62.0` adds List
Operator payload semantics, `1.63.0` closes the existing core Workflow success
payload schemas, `1.64.0` closes the remaining Ontology and HumanTask success
payload schemas, `1.65.0` adds exact Connector revision revocation plus closed
Connector success payloads, and `1.66.0` adds the safe terminal-indeterminate
attempt recovery surface described above. Contract `1.67.0` adds exact
Connector cancellation-compensation policy semantics, and `1.68.0` adds the
closed Agent failure-route values described above.

## Contract completeness

Every public REST operation must declare all of the following:

- one globally unique and stable `operationId`;
- a human-readable `summary` and `description`;
- at least one tag from the documented top-level tag catalog;
- explicit security, including `security: []` for a public operation;
- all path, query, and required header parameters with descriptions, bounds,
  formats, and examples or defaults;
- an explicit request-body media type, closed schema, description, and example
  for every operation that accepts a body;
- all successful and expected error statuses;
- the response envelope, response media type, and reusable examples; and
- `x-a3s-response-data`, authentication, idempotent-replay, stability, and
  contract-version metadata.

The completeness test walks the entire resolved route table. Adding a mutation
without a request schema fails OpenAPI generation rather than publishing an
unconstrained object.

## Authentication

Authenticated operations use an A3S API token:

```http
Authorization: Bearer a3s_<64-lowercase-hex-digits>
```

The OpenAPI `bearerAuth` scheme describes this requirement, while each
operation declares security explicitly. Public health, bootstrap, callback,
webhook, and node-enrollment routes use `security: []` as appropriate to their
own bounded authentication protocol.

Tokens are tenant-bound. Authorization is evaluated before idempotent replay,
and revocation takes effect immediately. Credential creation and enrollment
inputs are marked `writeOnly`; plaintext credentials are never represented in
normal response schemas.

## Request correlation and replay

Every mutation that supports replay requires a caller-owned
`idempotency-key` header:

```http
idempotency-key: provision-production-20260822-001
```

The same key may be reused only for the same logical request. A replay returns
the original authoritative result and does not repeat side effects. Mutations
that use optimistic concurrency additionally document either an
`x-a3s-expected-version` header or an `expectedVersion` field.

Every response includes:

- `x-request-id`, a UUID used for correlation; and
- `x-a3s-api-contract-version`, the exact contract used by the server.

## JSON envelopes

Normal JSON success responses use this shape:

```json
{
  "code": 200,
  "message": "Success or idempotent replay",
  "data": {},
  "requestId": "00000000-0000-4000-8000-000000000001",
  "timestamp": "2026-08-22T00:00:00Z"
}
```

JSON errors use one stable transport and business-code shape:

```json
{
  "code": 409,
  "statusCode": "CONFLICT",
  "message": "The requested state transition conflicts with current state.",
  "details": {},
  "requestId": "00000000-0000-4000-8000-000000000001",
  "timestamp": "2026-08-22T00:00:00Z"
}
```

The HTTP status and `code` match. `statusCode` is the stable business error
code. Error details must not expose credentials, secret values, signed webhook
bodies, or provider authorization material.

Git Smart HTTP, node enrollment, redirects, and server-sent event streams use
their documented raw media types where an envelope would violate the protocol.

## Pagination and streams

Bounded list operations document a `limit` and, where supported, an opaque
`cursor`. Callers must return the cursor unchanged; its encoding is not a
public contract. A missing `nextCursor` or a `null` value means the page is
terminal.

Server-sent event operations use `text/event-stream`. Resume with the
documented cursor or `Last-Event-ID` value only after the preceding event was
processed completely. Clients must tolerate reconnects and duplicate delivery
at the transport boundary.

## A3S ACL request documents

Product configuration is A3S ACL. Operations that accept a native ACL document
declare `application/vnd.a3s.acl`; JSON wrappers use fields such as
`definitionAcl` only when the owning application contract requires additional
typed correlation data. Parse and generate these documents with `a3s-acl`.

ACL examples in the OpenAPI document are illustrative syntax. The field bounds,
schema identifiers, semantic validation, and canonical digest remain owned by
the domain identified by the operation.

Notification alert-policy responses use a required discriminated `target`.
Alert-policy v1 returns `{ "kind": "environment", ... }`; v2 returns
`{ "kind": "node", ... }`. The legacy `projectId` and `environmentId`
properties remain required nullable response fields: they retain their v1
values and are `null` for v2. The canonical ACL, `definitionSchema`, and digest
remain authoritative.

## Versioning and compatibility

The REST major version is encoded in the path and remains `/api/v1`. The
independent semantic contract version appears in `info.version`,
`x-a3s-api-contract-version`, the TypeScript client constant, and every JSON
response header.

- additive or documentation-semantic changes require a newer minor or patch
  contract version;
- an operation, response status, media type, or response field cannot be
  removed from `v1`;
- a new required parameter or a narrower documented input is rejected;
- deprecation requires a live replacement operation and at least 180 days
  before sunset; and
- changing the REST major requires a separate versioned contract and route
  prefix.

Contract `1.48.0` replaces legacy unconstrained request placeholders with the
closed schemas that the existing Rust DTOs already enforced at runtime. Each
such schema carries
`x-a3s-contract-correction: documents-existing-runtime-validation`. The
compatibility checker recognizes that exact, reviewable correction only when
the previous schema was wholly unconstrained; it does not permit narrowing a
previously documented field contract.

## Updating the snapshot

From the Cloud repository root:

```bash
A3S_CLOUD_UPDATE_OPENAPI=1 cargo test -p a3s-cloud-control-plane \
  committed_openapi_snapshot_matches_the_resolved_route_contract --lib
cargo test -p a3s-cloud-control-plane api_contract_tests --lib
python3 -m unittest discover -s tools/api-contract -p 'test_*.py'
```

On PowerShell, set `$env:A3S_CLOUD_UPDATE_OPENAPI = '1'` for the generation
test and remove it immediately afterward. Review the JSON diff together with
the route, DTO, client, documentation, and contract-version changes.
