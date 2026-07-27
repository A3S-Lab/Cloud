# A3S Cloud Management MCP

## Product boundary

The A3S Cloud management MCP endpoint is an authenticated presentation surface
over existing Cloud application commands and queries. It is not an A0 hosted
MCP asset, a second control plane, a database client, or a path to a node.

The first `C0.2` slice exposes a deliberately small core-resource catalog. It
proves the transport, authorization, tenant, idempotency, and response
boundaries before additional existing Cloud operations are admitted as tools.

## Transport contract

The endpoint is `POST /api/v1/mcp` and implements stateless Streamable HTTP for
MCP protocol version `2025-06-18`.

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
| `a3s_cloud_projects_create` | Command | `project:write` |
| `a3s_cloud_environments_create` | Command | `environment:write` |

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

## Conformance

The dedicated `C0.2` scenario in
[`tools/c0-conformance`](../tools/c0-conformance/README.md) boots the production
control-plane binary with the shipped A3S ACL configuration and digest-pinned
PostgreSQL 17. It proves administrator and `cloud:read` catalogs, denies a
hidden mutation without a database write, replays one REST Project command
through MCP using the same durable idempotency record, returns the same `404`
business-error contract for foreign and missing Projects, and observes token
revocation on the next MCP request. The persistence check requires the expected
Token digests, read-only scope, revocation, Project rows, and zero plaintext
credentials in responses, logs, evidence, or the PostgreSQL dump. Production
persistence reaches PostgreSQL only through A3S ORM repositories.

## Current limits

`C0.2` remains in progress. The next slices expand the curated catalog over
existing non-secret operational commands and queries. OAuth 2.1 discovery and
consent follow only after the token-scoped confused-deputy gate. Destructive
operations, Secret material, exec, terminal access, server-side sessions, and
JSON-RPC batching are not exposed by this slice.
