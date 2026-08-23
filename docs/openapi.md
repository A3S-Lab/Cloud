# A3S Cloud OpenAPI Contract

A3S Cloud publishes its complete REST contract as OpenAPI 3.0.3. The committed
snapshot and the document served by the control plane are the same bytes:

- repository snapshot: [`openapi/v1.json`](../openapi/v1.json);
- public runtime endpoint: `GET /api/v1/openapi.json`; and
- API base path: `/api/v1`.

The contract is generated from the resolved production route table. A snapshot
test rejects drift between routes and the committed document, and the
compatibility checker rejects undocumented or incompatible changes.

The current semantic contract version is `1.59.0`.

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
