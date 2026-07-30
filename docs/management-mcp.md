# A3S Cloud Management MCP

## Product boundary

The A3S Cloud management MCP endpoint is an authenticated presentation surface
over existing Cloud application commands and queries. It is not an A0 hosted
MCP asset, a second control plane, a database client, or a path to a node.

The initial `C0.2` slice established the transport, authorization, tenant,
idempotency, and response boundaries with core Project, Environment, and search
tools. The operational-read slice adds Node, Operation, Workload, Deployment,
Route, and BuildRun queries without adding another business or persistence
path. The observability-read slice adds bounded Workload and BuildRun log pages
and signed BuildRun evidence through the same application queries and REST
response projections. The operational-mutation slice adds five replay-safe
Workload, Deployment, and BuildRun commands through the existing application
handlers and REST response projections.

## Transport contract

The endpoint is `POST /api/v1/mcp` and implements a sessionless deployment of
the initialization-based MCP protocol version `2025-06-18`.

This is the verified `C0.2` compatibility baseline. It is not a claim of
modern `2026-07-28` MCP conformance: the modern protocol uses per-request
metadata instead of `initialize`, requires `server/discover`, and has no
protocol-level sessions.

- Requests and immediate responses use raw JSON-RPC 2.0 with
  `Content-Type: application/json`.
- Clients advertise both `application/json` and `text/event-stream` in
  `Accept`, even though the stateless first slice returns immediate JSON.
- After initialization, every request carries
  `MCP-Protocol-Version: 2025-06-18`.
- The endpoint emits no `MCP-Session-Id`; `GET /api/v1/mcp` returns `405`.
- JSON-RPC batches are rejected. Each HTTP request carries one message.
- Browser-originated requests are accepted only when `Origin` matches `Host`.
- Transport responses are not wrapped in the REST envelope. A successful or
  failed tool execution carries the same REST success or business-error
  envelope in both `structuredContent` and text content.

The endpoint is hidden from the REST OpenAPI document because JSON-RPC and MCP
tool schemas, rather than REST operations, define this transport contract.

## Planned modern protocol migration

`C0.2m` migrates this same management presentation surface to MCP revision
`2026-07-28`:

- remove the `initialize` flow;
- require protocol version and client capabilities in every request's `_meta`;
  validate recommended `clientInfo` when present without treating it as an
  authenticated identity;
- require `MCP-Protocol-Version`, `Mcp-Method`, and applicable `Mcp-Name`
  headers and reject header/body mismatches;
- implement `server/discover`;
- retain one POST per JSON-RPC request, request-level authentication, Origin
  validation, no `Mcp-Session-Id`, and `405` for GET and DELETE; and
- rerun the exact tool visibility, authorization, revocation, idempotency,
  PostgreSQL, malformed-request, and redaction gates.

This migration does not change Cloud application commands, queries, scopes,
tool catalogs, or persistence. It is also separate from product gate `MCP0`,
which deploys tenant MCP AssetReleases as Runtime Services behind Gateway.
Until `C0.2m` passes, clients must use the verified `2025-06-18` flow below.

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

## Client flow

Initialize the protocol with an API token:

```bash
curl --request POST "https://cloud.example.com/api/v1/mcp" \
  --header "Authorization: Bearer ${A3S_CLOUD_TOKEN}" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --data '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "initialize",
    "params": {
      "protocolVersion": "2025-06-18",
      "capabilities": {},
      "clientInfo": {"name": "operator-client", "version": "1.0.0"}
    }
  }'
```

Then list only the tools visible to that token:

```bash
curl --request POST "https://cloud.example.com/api/v1/mcp" \
  --header "Authorization: Bearer ${A3S_CLOUD_TOKEN}" \
  --header "Content-Type: application/json" \
  --header "Accept: application/json, text/event-stream" \
  --header "MCP-Protocol-Version: 2025-06-18" \
  --data '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
```

Mutation tools require a caller-owned idempotency key in their arguments. A
REST call and an MCP call with the same command input and key resolve to the
same durable idempotency identity and replay projection.

## Bounded observability reads

`a3s_cloud_workload_logs_get` accepts `workloadId`, `revisionId`, and optional
`cursor`, `limit`, and `stream` arguments. `a3s_cloud_build_run_logs_get`
accepts `buildRunId` with the same optional page arguments. Cursors use the
opaque REST-compatible `v1:<sequence>` form, the default page contains 100
records, the maximum is 256, and `stream` is either `stdout` or `stderr`.
Responses reuse the REST log DTOs and retain explicit gap records and the next
opaque cursor.

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

The dedicated `C0.2` scenario in
[`tools/c0-conformance`](../tools/c0-conformance/README.md) boots the production
control-plane binary with the shipped A3S ACL configuration and digest-pinned
PostgreSQL 17. It proves the exact 23-tool administrator and 16-tool
`cloud:read` catalogs and their read-only, destructive, idempotent, and
closed-world annotations; denies a hidden mutation without a database write;
replays one REST Project command through MCP using the same durable idempotency
record; and returns the same `404` business-error contract for foreign and
missing Projects. It creates a real Environment, exercises all five operational
list tools, verifies all eight detail/log/evidence tools and all five commands
return bounded `NOT_FOUND` envelopes for missing resources, and rejects invalid
read and command arguments. It also creates one Workload from A3S ACL, stops it
through MCP, proves exact replay, and observes token revocation on the next MCP
request. The persistence check requires the expected Token digests, read-only
scope, revocation, Project, Environment, stopped Workload, and idempotency rows,
plus zero plaintext credentials in responses, logs, evidence, or the PostgreSQL
dump. Production persistence reaches PostgreSQL only through A3S ORM
repositories.

## Current limits

`C0.2` is verified for `2025-06-18`; `C0.2m` remains planned. OAuth 2.1
discovery and consent follow only after the token-scoped confused-deputy gate.
Secret material, exec, terminal access, server-side sessions, live log
streams, and JSON-RPC batching are not exposed by this slice. No additional
mutation is admitted without its existing scope, idempotency contract, tenant
boundary, and audit behavior.

## Protocol references

- [MCP versioning and compatibility, revision 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [MCP Streamable HTTP, revision 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
- [MCP server discovery](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
