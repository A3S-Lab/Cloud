# 0075: Persist and expose UserFile through one lifecycle authority

Status: Accepted

## Context

Decision 0030 freezes the canonical `cloud.user-file.v1` A3S ACL, the
`UserFile` state machine, exact upload and scan receipts, and the streaming
adapter over the deployment's shared immutable-object client. It deliberately
leaves quota, durable metadata, authorization, idempotency, audit, Outbox, and
maintained management interfaces to the next gate.

Those concerns cannot be implemented as independent services. Reserving bytes
without persisting the aggregate leaks quota. Persisting a transition without
its event makes cleanup and downstream observation incomplete. Replaying
before authorization exposes tenant state. A Files-specific uploader, object
provider, scan table, cleanup queue, or presentation-owned repository would
create a second authority for behavior already owned by the aggregate or the
shared platform mechanisms.

## Decision

Files has one Application service and two inward interfaces:

- `IUserFileRepository` owns the complete metadata consistency boundary; and
- `IUserFileObjectStore` owns streaming `AsyncRead` writes and exact immutable
  object verification while hiding provider details.

`UserFileApplicationService` authorizes the Project before validation or
idempotency replay. Organization quota reads additionally require
organization-wide authority and are concealed from restricted Principals.
Reservation parses the canonical admission ACL only through `a3s-acl`, proves
that its organization and project equal the requested scope, and uses the ACL
digest in the idempotency request. Lifecycle transitions use optimistic
aggregate versions and stable action-specific idempotency scopes.

Migration `170` creates only `user_files` and
`user_file_organization_quotas`. The repository serializes an Organization's
allocation row and commits all of the following in one A3S ORM transaction:

- the validated `UserFile` aggregate projection;
- quota reservation or release;
- one metadata-only lifecycle event in the shared Outbox;
- one shared audit record; and
- the shared idempotency result.

`(organization_id, user_file_id)` is the aggregate identity used by the
Outbox; `project_id` is its immutable authorization scope rather than part of
a second identity. Upload identity is likewise unique within the Organization.
Both PostgreSQL and the in-memory adapter enforce these same rules.

Quota is reserved before bytes may be accepted. It is released only when an
unused reservation expires or a live file is tombstoned; rejection alone does
not silently remove evidence. The 50 GiB initial limit is a fixed versioned
domain admission default for a newly observed Organization, not a mutable
environment/YAML/JSON configuration path. Once created, the database row is
authoritative and is never rewritten by a later default change.
Quota values are bounded to the largest JavaScript-safe integer in the domain
and PostgreSQL constraint so REST, OpenAPI, the TypeScript client, and storage
cannot disagree about integer precision.

`cleanup_due_at` is derived from the aggregate's state and canonical retention
deadline. It is persisted only as a checked query projection and carried by
the same lifecycle event. Files does not add a cleanup table, deletion queue,
scheduler, retry rail, or raw object-removal API. A later cleanup executor must
consume this authority and preserve lifecycle consistency rather than infer
deletion from object inventory.

The maintained management surface is exact:

| Capability | REST | TypeScript client | CLI | Management MCP | Scope |
| --- | --- | --- | --- | --- | --- |
| Reserve metadata and quota | `POST /organizations/{organizationId}/projects/{projectId}/user-files` | `reserveUserFile` | `user-files reserve` | `a3s_cloud_user_files_reserve` | `file:write` |
| List bounded projections | `GET /organizations/{organizationId}/projects/{projectId}/user-files` | `listUserFiles` | `user-files list` | `a3s_cloud_user_files_list` | `cloud:read` |
| Get one projection | `GET /organizations/{organizationId}/projects/{projectId}/user-files/{userFileId}` | `getUserFile` | `user-files get` | `a3s_cloud_user_files_get` | `cloud:read` |
| Tombstone and release quota | `POST /organizations/{organizationId}/projects/{projectId}/user-files/{userFileId}/tombstone` | `tombstoneUserFile` | `user-files tombstone` | `a3s_cloud_user_files_tombstone` | `file:write` |
| Read organization quota | `GET /organizations/{organizationId}/user-file-quota` | `getUserFileQuota` | `user-file-quota get` | `a3s_cloud_user_file_quota_get` | `cloud:read` |

REST/OpenAPI `1.77.0` and Management MCP dispatch the same commands and
queries and reuse the same DTO projections. The public request body carries
only canonical bounded ACL or optimistic version data. No public route or MCP
tool carries file bytes.

Internal upload, scan, and unused-reservation expiry commands retain the same
Application service and interfaces, but they are not advertised as available
until their owning provider/execution and cleanup gates have retained evidence.
Knowledge consumes only an admitted typed reference and remains the sole owner
of document, chunk, index, retrieval, and KnowledgePipeline semantics.

## Consequences

- Concurrent reservation cannot over-allocate an Organization because quota
  and aggregate creation share one locked transaction.
- A committed mutation cannot omit its audit, Outbox, idempotency, or quota
  effect, and a replay cannot repeat them.
- REST, client, CLI, and Management MCP are adapters over the same CQRS
  authority; none owns lifecycle or persistence behavior.
- Files has no second upload aggregate, byte store, object provider, scanner
  state store, cleanup queue, event rail, audit writer, or idempotency store.
- `K0.1-C2` completes the management and persistence slice only. Public byte
  transfer, live scan execution, cleanup execution, retained PostgreSQL
  cross-surface certification, Knowledge, and KnowledgePipeline availability
  remain separate gates.
