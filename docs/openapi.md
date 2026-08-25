# A3S Cloud OpenAPI Contract

A3S Cloud publishes its complete REST contract as OpenAPI 3.0.3. The committed
snapshot and the document served by the control plane are the same bytes:

- repository snapshot: [`openapi/v1.json`](../openapi/v1.json);
- public runtime endpoint: `GET /api/v1/openapi.json`; and
- API base path: `/api/v1`.

The contract is generated from the resolved production route table. A snapshot
test rejects drift between routes and the committed document, and the
compatibility checker rejects undocumented or incompatible changes.

The current semantic contract version is `1.63.0`.

Contract `1.63.0` closes every core Workflow success payload already returned
by the control plane. Goal, Plan revision, node-catalog, run, cancellation,
wait, output, variable-inspection, diagnostics, and history operations now
reference reusable closed schemas through the standard response envelope.
The schemas preserve the existing JSON fields and routes while documenting
their UUIDs, digests, finite enums, nullable projections, collection bounds,
typed failure evidence, diagnostic statistics, and exact nested resources.
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
Operator payload semantics, and `1.63.0` closes the existing core Workflow
success payload schemas as described above.

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
