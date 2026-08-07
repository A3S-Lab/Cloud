# A3S Cloud Management MCP

## Product boundary

The A3S Cloud management MCP endpoint is an authenticated presentation surface
over existing Cloud application commands and queries. It is not an A0 hosted
MCP asset, a second control plane, a database client, or a path to a node.

The initial `C0.2` slice established the transport, authorization, tenant,
idempotency, and response boundaries with core Project, Environment, and search
tools. The operational-read slice adds Node, Operation, Workload, Deployment,
Route, and BuildRun queries without adding another business or persistence
path. The observability-read slice adds bounded Workload log pages, explicit
BuildRun-log unavailability, and signed BuildRun evidence through the same
application queries and REST response projections. The operational-mutation slice adds five replay-safe
Workload, Deployment, and BuildRun commands through the existing application
handlers and REST response projections.
The backend `W0.2` slice adds seven Ontology create/read/revise/revision/diff
tools over the same Workflow command/query handlers. It does not introduce an
MCP-specific Ontology store, migration policy, ACL parser, or graph database.

## Transport contract

The endpoint is `POST /api/v1/mcp` and implements a sessionless deployment of
MCP protocol version `2026-07-28`. `C0.2m` changes only this presentation
adapter; it retains the verified `C0.2` application, authorization,
idempotency, persistence, and audit boundaries.

- Requests and immediate responses use raw JSON-RPC 2.0 with
  `Content-Type: application/json`.
- Clients advertise both `application/json` and `text/event-stream` in
  `Accept`, even though the stateless first slice returns immediate JSON.
- Every request carries matching protocol version and client-capability
  metadata under `params._meta`, `MCP-Protocol-Version`, and `Mcp-Method`.
  `tools/call`, `prompts/get`, and `resources/read` also carry a matching
  `Mcp-Name`; the Base64 sentinel form is accepted for values that cannot be
  represented safely as an HTTP field value.
- `clientInfo` is optional protocol metadata and is validated when present. It
  is never used as an authenticated identity.
- The endpoint ignores legacy `Mcp-Session-Id` input and never creates or emits
  protocol session state; `GET` and `DELETE /api/v1/mcp` return `405`.
- JSON-RPC batches are rejected. Each HTTP request carries one message.
- Notifications and the legacy `initialize` method are rejected.
- Browser-originated requests are accepted only when `Origin` matches `Host`.
- Transport responses are not wrapped in the REST envelope. A successful or
  failed tool execution carries the same REST success or business-error
  envelope in both `structuredContent` and text content.
- Every successful result is complete and includes `resultType: "complete"`
  plus bounded server metadata. `server/discover` returns the supported
  version, tools capability, private cache scope, and zero discovery TTL.

The endpoint is hidden from the REST OpenAPI document because JSON-RPC and MCP
tool schemas, rather than REST operations, define this transport contract.

## Modern protocol boundary

`C0.2m` removes the legacy initialization state without creating a replacement
session or a second management mechanism. Unsupported versions return JSON-RPC
error `-32022` with the supported and requested versions. Missing or invalid
required body metadata returns HTTP `400` with `-32602`; missing or mismatched
transport headers return HTTP `400` with `-32020`. Unknown methods return HTTP
`404` with JSON-RPC error `-32601`.

The migration does not change Cloud application commands, queries, scopes,
tool catalogs, A3S ORM persistence, or audit behavior. It is also separate
from product gate `MCP0`, which deploys tenant MCP AssetReleases as Runtime
Services behind Gateway.

## Authentication and authorization

The endpoint uses the same bounded API tokens as REST and CLI. Authentication
loads the current token, organization claim, and effective scopes on every
request through the Identity repository. Revocation therefore takes effect on
the next request without an MCP session, cache, Redis, or separate credential
store.

The organization is always derived from the authenticated principal. No tool
accepts an organization identifier, and unknown input properties are rejected
before a command or query runs. Project identifiers remain subject to the same
tenant-aware application queries used by REST.

`cloud:read` is the delegable read-only scope. Organization reads predate
explicit read scopes, so every authenticated tenant token retains the same
baseline reads; `cloud:read` lets a `token:write` issuer create a token with no
mutation capability without granting a new resource read. Existing mutation
scopes control mutation tool visibility and invocation independently:

| Tool | Kind | Required mutation scope |
| --- | --- | --- |
| `a3s_cloud_projects_list` | Query | None |
| `a3s_cloud_environments_list` | Query | None |
| `a3s_cloud_memberships_list` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_memberships_get` | Administrator query | `identity:write` plus organization administrator role |
| `a3s_cloud_service_memberships_create` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_memberships_change_role` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_memberships_revoke` | Administrator command | `identity:write` plus organization administrator role |
| `a3s_cloud_ontologies_list` | Query | None |
| `a3s_cloud_ontologies_get` | Query | None |
| `a3s_cloud_ontology_revisions_list` | Query | None |
| `a3s_cloud_ontology_revisions_get` | Query | None |
| `a3s_cloud_ontology_revisions_diff` | Query | None |
| `a3s_cloud_ontologies_create` | Command | `ontology:write` |
| `a3s_cloud_ontologies_revise` | Command | `ontology:write` |
| `a3s_cloud_search` | Query | None |
| `a3s_cloud_nodes_list` | Query | None |
| `a3s_cloud_nodes_get` | Query | None |
| `a3s_cloud_operations_list` | Query | None |
| `a3s_cloud_workloads_list` | Query | None |
| `a3s_cloud_workloads_get` | Query | None |
| `a3s_cloud_workload_logs_get` | Query | None |
| `a3s_cloud_deployments_get` | Query | None |
| `a3s_cloud_routes_list` | Query | None |
| `a3s_cloud_routes_get` | Query | None |
| `a3s_cloud_build_runs_list` | Query | None |
| `a3s_cloud_build_runs_get` | Query | None |
| `a3s_cloud_build_run_logs_get` | Query | None |
| `a3s_cloud_build_evidence_get` | Query | None |
| `a3s_cloud_projects_create` | Command | `project:write` |
| `a3s_cloud_environments_create` | Command | `environment:write` |
| `a3s_cloud_workloads_stop` | Command | `workload:write` |
| `a3s_cloud_workloads_rollback` | Command | `workload:write` |
| `a3s_cloud_deployments_cancel` | Command | `workload:write` |
| `a3s_cloud_build_runs_cancel` | Command | `build:write` |
| `a3s_cloud_build_runs_retry` | Command | `build:write` |

A tool that is unavailable to the current principal is absent from
`tools/list` and is indistinguishable from an unknown tool when invoked. The
scope check is repeated during invocation; hiding a tool is never the
authorization boundary.

`a3s_cloud_build_run_logs_get` remains discoverable for API compatibility but
returns the standard `503 Service Unavailable` business envelope until Box
exposes an authoritative durable build-log contract. It does not return an
empty success page and does not reuse Workload or Runtime logs.

## Client flow

Discover the server with an API token. The client sends protocol metadata on
this and every later request:

```bash
curl --request POST "https://cloud.example.com/api/v1/mcp" \
  --header "Authorization: Bearer ${A3S_CLOUD_TOKEN}" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --header "MCP-Protocol-Version: 2026-07-28" \
  --header "Mcp-Method: server/discover" \
  --data '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "server/discover",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {
          "name": "operator-client",
          "version": "1.0.0"
        }
      }
    }
  }'
```

Then list only the tools visible to that token:

```bash
curl --request POST "https://cloud.example.com/api/v1/mcp" \
  --header "Authorization: Bearer ${A3S_CLOUD_TOKEN}" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --header "MCP-Protocol-Version: 2026-07-28" \
  --header "Mcp-Method: tools/list" \
  --data '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/list",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
      }
    }
  }'
```

For `tools/call`, also send `Mcp-Name` with the exact tool name. Header and
body metadata are integrity checks, not authorization inputs; the bearer token
continues to determine the tenant and visible tools on every request.

Mutation tools require a caller-owned idempotency key in their arguments. A
REST call and an MCP call with the same command input and key resolve to the
same durable idempotency identity and replay projection.

## Versioned Ontology lifecycle

`a3s_cloud_ontologies_create` accepts `projectId`, at most 1 MiB of closed A3S
ACL, and `idempotencyKey`. `a3s_cloud_ontologies_revise` accepts `ontologyId`,
ACL, a positive `expectedVersion`, `idempotencyKey`, and an optional portable
`migrationRuleId`. The optional rule is not a second policy source: a breaking
deterministic diff is admitted only when that ID resolves to a rule of kind
`migration` in the target ACL. Non-breaking changes derive the `compatible`
policy from the same diff.

The five read tools list/get Ontologies, list/get immutable revisions, and diff
two exact revisions. They return the same REST DTOs and canonical ACL as the
Workflow QueryBus. Create and revise return the same mutation envelope,
including diff and replay status. Historical replay projects the aggregate at
the accepted revision even when a later head exists. All seven tools use the
same PostgreSQL/A3S ORM repository, audit, Outbox, Search projection, tenant
guard, and application handlers as REST and CLI.

## Bounded observability reads

`a3s_cloud_workload_logs_get` accepts `workloadId`, `revisionId`, and optional
`cursor`, `limit`, and `stream` arguments. Cursors use the opaque
REST-compatible `v1:<sequence>` form, the default page contains 100 records,
the maximum is 256, and `stream` is either `stdout` or `stderr`. Responses reuse
the REST log DTOs and retain explicit gap records and the next opaque cursor.

`a3s_cloud_build_run_logs_get` accepts `buildRunId` and the same bounded
arguments so its public contract remains stable, but currently returns the
standard `503 Service Unavailable` envelope. Box has not yet exposed the
authoritative durable build-log records needed for a successful page.

`a3s_cloud_build_evidence_get` accepts only `buildRunId` and returns the same
signed SPDX/SLSA evidence projection as REST. The MCP surface performs no live
node read and exposes no SSE stream: retained log objects, metadata, and
evidence continue through the existing QueryBus handlers, A3S ORM repositories,
and configured object store. Existing ingestion redaction and tenant guards
remain authoritative.

## Replay-safe operational mutations

`a3s_cloud_workloads_stop`, `a3s_cloud_deployments_cancel`, and
`a3s_cloud_build_runs_cancel` are marked destructive. Workload rollback and
BuildRun retry are non-destructive recovery actions. All five are non-read-only,
idempotent, closed-world tools.

Every invocation requires `idempotencyKey` with 1 through 255 header-safe UTF-8
bytes. Workload stop also requires `workloadId`; rollback requires `workloadId`
and `sourceRevisionId`; Deployment cancel requires `deploymentId`; and both
BuildRun commands require `buildRunId`. Unknown properties, missing fields,
invalid UUIDs, empty or oversized keys, and newline-bearing keys fail as
JSON-RPC invalid parameters before command dispatch.

The adapter derives the organization from the authenticated principal, adds
the request ID and current timestamp where the existing command requires them,
and dispatches through `CommandBus`. It does not read a repository, SQL, Redis,
an object store, or a node. Initial accepted commands return code `202`; exact
replays return code `200` and `replayed: true` in the standard envelope.

## Conformance

The dedicated `C0.2m` scenario in
[`tools/c0-conformance`](../tools/c0-conformance/README.md) boots the production
control-plane binary with the shipped A3S ACL configuration and digest-pinned
PostgreSQL 17. It first proves `server/discover`, per-request version and
client metadata, exact transport-header matching, legacy initialization
removal, and unsupported-version errors. The verified pre-extension evidence
proved the exact 23-tool administrator and 16-tool `cloud:read` catalogs. The
current expanded runner requires exact 35-tool administrator and 21-tool
`cloud:read` catalogs and their read-only, destructive, idempotent, and
closed-world annotations; denies a hidden mutation without a database write;
replays one REST Project command through MCP using the same durable idempotency
record; and returns the same `404` business-error contract for foreign and
missing Projects. It also creates an Ontology through REST, replays it through
MCP, exercises all seven Ontology tools with a read-only token where
applicable, rejects a breaking revision without its target migration rule,
publishes the explicit migration, and proves historical replay after later
revisions. It creates a real Environment, exercises all five operational
list tools, verifies all eight detail/log/evidence tools and all five commands
return bounded `NOT_FOUND` envelopes for missing resources, and rejects invalid
read and command arguments. It also creates one Workload from A3S ACL, stops it
through MCP, proves exact replay, and observes token revocation on the next MCP
request. The persistence check requires the expected Token digests, read-only
scope, revocation, Project, Environment, stopped Workload, and idempotency rows,
plus zero plaintext credentials in responses, logs, evidence, or the PostgreSQL
dump. Production persistence reaches PostgreSQL only through A3S ORM
repositories.

The expanded focused catalog, permission, lifecycle, migration, and replay
tests pass. The updated clean PostgreSQL/A3S Box scenario and its Ontology row,
revision, and idempotency assertions must pass before `W0.2` is verified.

## Current limits

`C0.2m` is verified by the clean Linux PostgreSQL/A3S Box conformance gate and
retains bearer-token authentication. OAuth 2.1 discovery and consent follow
only after the token-scoped confused-deputy gate.
Secret material, exec, terminal access, server-side sessions, live log
streams, and JSON-RPC batching are not exposed by this slice. No additional
mutation is admitted without its existing scope, idempotency contract, tenant
boundary, and audit behavior.

## Protocol references

- [MCP versioning and compatibility, revision 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP Streamable HTTP, revision 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP server discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
