# A3S Cloud

<p align="center">
  <strong>Self-Hosted Cloud for Applications and AI Workloads</strong>
</p>

<p align="center">
  <em>Deploy, route, observe, update, and roll back workloads on infrastructure you own</em>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#configuration">Configuration</a> •
  <a href="#platform-model">Platform Model</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#delivery-roadmap">Delivery Roadmap</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Cloud** is a self-hosted platform for deploying applications and A3S
workloads to operator-owned Linux infrastructure. Organizations, projects, and
environments define its tenancy boundary. PostgreSQL stores desired state, A3S
Flow advances durable operations, and node agents converge A3S Runtime resources
with the requested state.

Cloud is designed around observable convergence rather than request-time
orchestration. An accepted command commits intent and returns an operation. The
operation continues across process restarts, records its progress, and completes
only after the relevant infrastructure reports the requested state.

Cloud separates desired state from execution. The control plane persists intent
before dispatching work, while Linux nodes establish outbound mTLS connections,
apply provider-neutral A3S Runtime requests, and report observations back to the
control plane. API latency is therefore independent from deployment duration,
and recovery starts from persisted state instead of process memory.

### Operation model

```text
API command
    │
    ├── commit desired state + outbox fact in PostgreSQL
    ├── start or locate the durable A3S Flow operation
    └── return the authoritative operation identity
             │
             v
       reconciler rebuilds projections
             │
             ├── query snapshot
             └── authenticated SSE update
```

## Features

- **Boot-Based Control Plane**: Use A3S Boot modules, typed providers, CQRS,
  global authentication, response interception, health indicators, and OpenAPI
- **Tenant Boundaries**: Organize control-plane state by organization, project,
  and isolated environment
- **Scoped API Tokens**: Bootstrap the first organization atomically, issue
  expiring tokens with bounded scopes, and revoke them without storing plaintext
  credentials
- **Idempotent Commands**: Require idempotency keys for mutations and reject
  payload conflicts instead of duplicating state
- **Durable Operations**: Persist operation requests in PostgreSQL, execute them
  through A3S Flow, and rebuild query projections after interruption
- **Transactional Events**: Commit domain facts with business state and relay
  them through either an in-memory A3S Event provider or NATS JetStream
- **Independent Timing Policies**: Configure operation reconciliation, outbox
  polling, leases, publishing, and retry backoff independently
- **Outbound Node Control**: Enroll Linux nodes, rotate their certificates,
  lease idempotent commands, and recover command journals without inbound node
  ports
- **Atomic Gateway Snapshots**: Validate and transactionally reload complete
  A3S Gateway ACL snapshots with compare-and-swap revisions, durable local
  state, and command-bound acknowledgements
- **Convergent Edge Routes**: Bind each hostname/path to one healthy immutable
  workload revision, compile the complete Gateway scope deterministically, and
  activate a route only after the exact command, revision, and digest are
  acknowledged
- **Managed Gateway TLS**: Verify exact or one-label wildcard domain claims,
  use bounded system-DNS TXT ownership checks in production, bind certificate
  intent into the complete snapshot digest, support Vault signing and
  serial-based provider revocation through a dedicated Gateway PKI role,
  reconcile renewal and revoked ownership through exact complete-snapshot
  acknowledgements, revoke obsolete provider serials only after no active
  route references them, issue only public material over node mTLS, and keep
  generated private keys on the Gateway node
- **Encrypted Secret Resources**: Create tenant-scoped Secret identities,
  rotate immutable encrypted versions through local AES-GCM or Vault Transit,
  bind exact versions to workload environment, file, or registry-credential
  targets, materialize only for an authoritative registry challenge or the
  assigned node over mTLS, and use them at the registry, Docker create, or
  authenticated image-pull boundary without placing plaintext in desired
  state, Runtime state, commands, or events; a dedicated
  Linux/PostgreSQL/Docker gate exercises production authorization and
  decryption at both private-registry resolution and node pull, plus real
  environment injection and `0400` tmpfs-file injection; after a committed
  rotation, a restart worker advances every matching binding on an active
  running workload in a new immutable, digest-pinned revision and checkpoints
  the causally linked deployment so process loss or concurrent workers cannot
  duplicate it; the isolated recovery gate also kills the Docker provider and
  agent during the rotated apply, then reattaches the exact container and
  completes the pending Runtime receipt without plaintext persistence
- **Runtime Observations**: Record provider capabilities, workload state,
  health, logs, and durable command acknowledgements from A3S Runtime
- **Durable Workload Logs**: Project active Runtime targets from the command
  journal, persist one bounded batch before mTLS upload, resume only after an
  exact receipt, project typed provider cursor-loss/source-disconnect gaps,
  redact bound Secret values at the Docker log boundary, and query verified
  immutable filesystem or S3-compatible chunk objects through tenant-scoped
  cursor pages or a bounded resumable SSE feed while a configurable worker
  deletes expired bodies and a second bounded worker compacts old tombstones
  into explicit sequence ranges without losing replay or ordering watermarks;
  the Linux acceptance gate persists real redacted stdout/stderr, reconstructs
  the persistence adapters, replays the exact batch, and reads it through REST
- **Digest-Pinned Deployments**: Resolve mutable OCI tags once, persist the
  resulting digest, schedule one eligible node, and activate only after real
  Runtime health evidence
- **Immutable One-Node Updates**: Accept a complete replacement template for
  an active workload, run the candidate on the previous Runtime node, preserve
  the active revision and routes until health and the exact Gateway
  acknowledgement succeed, then stop the previous revision with a
  deterministic command and wait for durable stopped-or-absent evidence
- **Manual Immutable Rollback**: Select an older successfully activated
  revision, clone its exact resolved and digest-pinned template into a new
  monotonically increasing generation, revalidate its Secret bindings, and run
  it through the same health, Gateway cutover, activation, and retirement path
- **Convergent Recovery**: Reattach after provider creation, recover a lost
  provider at the same generation, preserve the prior healthy revision on a
  failed or rejected update, survive process death after activation but before
  retirement dispatch, resume the deterministic cleanup, and drive cancellation
  through bounded cleanup
- **Clean-Host E0 Certification**: Build release binaries from exact clean
  Cloud and Runtime revisions, start pinned PostgreSQL and registry fixtures,
  A3S Gateway 1.0.12, the control plane, and one real outbound Docker node,
  then prove bootstrap, enrollment, digest-pinned deployment, managed TLS,
  ordered resumable logs, immutable update, cloned rollback, durable stop, and
  exact source and host cleanup without credential leakage
- **Operation Streaming**: Expose tenant-scoped snapshots and resumable
  server-sent events with stable content-derived event identifiers
- **Web Console**: Sign in with a session-scoped API token, select the active
  organization, project, and environment, inspect the authoritative deployment
  timeline plus route/certificate state, edit a complete immutable template
  with field-level differences, roll back to an eligible activated revision,
  locally dismiss terminal operation projections, and follow observed Runtime
  health plus bounded stdout/stderr records with explicit gaps

### Delivery capability matrix

| Area | Capability | State |
| --- | --- | --- |
| Runtime prerequisite | General Task and Service lifecycle with provider capability matching | Complete |
| Foundation | Identity, tenancy, PostgreSQL, Flow, outbox, projections, API, and web shell | Complete |
| Node control | Enrollment, node identity, outbound mTLS, command leases, and observations | Complete |
| Deployment | Digest-pinned OCI revisions, scheduling, apply, health, activation, stop, cancellation, recovery, one-node immutable replacement, and manual rollback with deterministic previous-revision retirement, including real process death after activation but before retirement dispatch | Complete (`E0` update and rollback slice) |
| Reachability | Route ownership, production DNS TXT ownership verification and explicit revocation, a Vault-backed production Gateway PKI adapter, managed TLS provisioning, automated renewal/revocation convergence, delayed obsolete-serial revocation, routed Gateway validation, complete snapshot publication, reload-before-acknowledgement process-death recovery, exact acknowledgement projection, and byte-preserving routed update and rollback cutover | Complete (`E0` slice) |
| Secrets | Encrypted tenant-scoped resources, immutable rotation/revocation, typed environment/file/registry-credential workload bindings, transient authenticated manifest resolution, assigned-node mTLS materialization, metadata-only APIs/events, reference-only durable state, authenticated private-image pulls, environment and `0400` tmpfs-file injection, post-commit automatic restart orchestration, concurrent replay/process-loss recovery, provider-and-agent-death recovery during rotated apply with exact container reattachment and receipt replay, causal checkpoints, and final durable-state plaintext scans are implemented; the production paths are exercised by the isolated PostgreSQL and Linux/Docker gates | Complete (`E0` slice) |
| Logs | Restart-safe bounded node shipping, typed provider cursor-loss/source-disconnect recovery, monotonic delivery rebasing, Docker-bound Secret redaction, PostgreSQL chunk/gap metadata, verified filesystem/S3-compatible chunk objects, cursor paging, resumable bounded SSE and a 500-record web window, tenant isolation, configurable body retention, bounded tombstone compaction, explicit provider/missing/corrupt/retained/compacted gaps, Docker provider-restart cursor continuity, control-plane object-before-receipt process-death recovery, filesystem/REST corruption projection, and real MinIO corruption rejection are implemented | Complete (`E0` slice) |
| Web operations | Authoritative deployment history, exact route/certificate projection, complete-template update differences and action, eligible manual rollback, operation lineage, and browser-local terminal cleanup | Complete (`E0` slice) |
| Release conformance | Exact clean Cloud/Runtime release build, one real outbound Linux/Docker node, A→B→cloned-A TLS cutover, ordered and resumable logs, durable stop, source-cleanliness checks, host-inventory equality, and credential scanning | Verified (`E0`) |
| Source delivery | Pinned Git revisions, isolated builds, OCI publication, provenance, and push-to-deploy | Planned (`G0`) |
| Developer workflows | Stack detection, web/worker/scheduled profiles, previews, monorepos, and closed Compose import through typed desired state | Planned (`P0`) |
| Control surfaces | Stable REST, Cloud CLI, management MCP, collaboration, notifications, audit, and bounded terminal access | Planned (`C0`) |
| Releases | Immutable Agent, MCP, and Skill publication through the common deployment path | Planned (`A0`) |
| Stateful platform | Databases, volumes, verified backup/restore, and stateful Compose mappings | Planned (`S0`) |
| Production scale | Replicas, multi-node placement, Gateway target sets, HA, and measured autoscaling | Planned (`H0`) |

## Quick Start

### Requirements

- Rust 1.85 or later
- PostgreSQL 17 or a compatible supported release
- A3S Gateway 1.0.12 or later
- Bun and Node.js 22 or later for the web console
- Docker for the first node Runtime provider and real deployment gates
- NATS JetStream only when the NATS event provider is enabled

### Run the control plane

Start the pinned local PostgreSQL and NATS profile, then run Cloud from this
repository directory. Database migrations are applied during startup.

```bash
docker compose \
  --env-file deploy/dev/.env.example \
  --file deploy/dev/compose.yaml \
  up --detach --wait

export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:a3s_cloud@127.0.0.1:54320/a3s_cloud"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"

cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

The default configuration listens on `127.0.0.1:8080` and uses the in-memory
event provider. Verify process and dependency health:

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
```

The API schema is available at
`http://127.0.0.1:8080/api/v1/openapi.json`.

### Bootstrap an organization

The caller creates the first API-token secret and must retain it. Cloud stores
only its SHA-256 digest. Token secrets use the `a3s_` prefix followed by 64
lowercase hexadecimal characters.

```bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: ${A3S_CLOUD_BOOTSTRAP_TOKEN}" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"${A3S_CLOUD_ADMIN_TOKEN}\",\"expiresAt\":null}"
```

Subsequent API requests use
`Authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}`. Every mutation also requires a
stable `idempotency-key` header.

### Update an active workload

Submit a complete replacement service template to the workload deployment
collection:

```text
POST /api/v1/organizations/{organization_id}/workloads/{workload_id}/deployments
```

For example:

```bash
curl --request POST \
  "http://127.0.0.1:8080/api/v1/organizations/${A3S_CLOUD_ORGANIZATION_ID}/workloads/${A3S_CLOUD_WORKLOAD_ID}/deployments" \
  --header "authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}" \
  --header "content-type: application/json" \
  --header "idempotency-key: workload-update-v2" \
  --data '{
    "template": {
      "artifact": {
        "uri": "ghcr.io/example/application:v2",
        "expectedDigest": null
      },
      "process": {
        "command": [],
        "args": [],
        "workingDirectory": null,
        "environment": {}
      },
      "secrets": [],
      "resources": {
        "cpuMillis": 500,
        "memoryBytes": 536870912,
        "pids": 256,
        "ephemeralStorageBytes": null
      },
      "ports": [
        {
          "name": "http",
          "containerPort": 8080
        }
      ],
      "health": {
        "portName": "http",
        "path": "/health",
        "intervalMs": 1000,
        "timeoutMs": 500,
        "healthyThreshold": 2,
        "unhealthyThreshold": 3,
        "stabilizationWindowMs": 2000
      }
    }
  }'
```

A new request returns `202`; an exact idempotent replay returns `200` and the
same revision, deployment, and operation identities. New deployments use the
`cloud.deployment@2` workflow. Version 1 remains registered only so operations
persisted before routed updates can replay.

Only an active running workload may be updated, and a workload may have only
one nonterminal deployment. The candidate is scheduled on the previous
Runtime node. Its health must converge before Cloud stages a Gateway cutover,
and the old route rows remain byte-identical until an `applied`
acknowledgement matches the exact node, command, Gateway revision, and snapshot
digest. An unhealthy candidate, a mismatched acknowledgement, or a rejected
reload leaves the previous route and active revision selected. After the exact
acknowledgement, Cloud swaps the route target, selects the candidate, marks its
deployment `retiring`, and issues the deterministic stop for the previous
Runtime revision. Durable stopped-or-absent evidence makes the deployment
terminal `active`, including after coordinator recovery. Cancellation is
accepted only before the deployment reaches `verifying`.

### Roll back an active workload

Select an older revision of the same active running workload:

```text
POST /api/v1/organizations/{organization_id}/workloads/{workload_id}/rollback
```

```bash
curl --request POST \
  "http://127.0.0.1:8080/api/v1/organizations/${A3S_CLOUD_ORGANIZATION_ID}/workloads/${A3S_CLOUD_WORKLOAD_ID}/rollback" \
  --header "authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}" \
  --header "content-type: application/json" \
  --header "idempotency-key: workload-rollback-v1" \
  --data "{\"revisionId\":\"${A3S_CLOUD_ROLLBACK_REVISION_ID}\"}"
```

The target must be older than the current active revision and must have a
successfully activated deployment. Missing or cross-workload revisions return
`404`; current, newer, failed, unresolved, or otherwise ineligible revisions
return `409`.

Rollback never reactivates the old revision identity. It clones the target's
exact resolved template and artifact digest into the next generation,
revalidates every immutable Secret binding, and creates a new
`cloud.deployment@2` operation whose input records
`rollbackSourceRevisionId`. The response also includes that field. Health,
routed Gateway acknowledgement, activation, and previous-Runtime retirement
then use the ordinary immutable update workflow. An exact idempotent replay
returns the original revision, deployment, and operation with `200`, even if
the workload state changes after the first request; a different rollback source
at the same key returns `409`.

Workload detail and list responses project each revision's complete camel-case
`requestedTemplate`. Secret bindings contain only immutable Secret
ID/version/target references; plaintext is never returned. Operation list
responses expose `rollbackSourceRevisionId` only for rollback-derived
operations. The web console uses these projections directly for update
differences, rollback eligibility, deployment lineage, and route/certificate
state. Clearing terminal operations is browser-local visibility state and never
deletes durable operation or audit records.

### Run the web console

```bash
cd web
bun install --frozen-lockfile
bun run dev
```

The development server listens on `127.0.0.1:3010` and proxies `/api` to
`http://127.0.0.1:8080`. Set `A3S_CLOUD_API_ORIGIN` to use another control-plane
origin, then sign in with the API token created during bootstrap.

## Configuration

Cloud validates a closed A3S ACL configuration at startup. Unknown fields and
unsafe timing relationships fail before the API or worker starts. The shipped
deployment and Edge policies are split across independent boundaries:

| Setting | Owns |
| --- | --- |
| `deployments.command_ttl_ms` | How long a leased node command remains valid |
| `deployments.runtime_apply_timeout_ms` | Runtime apply deadline carried by the command |
| `deployments.observation_poll_ms` | Poll interval while waiting for durable Runtime evidence |
| `deployments.convergence_timeout_ms` | End-to-end deadline for one deployment generation |
| `deployments.runtime_stop_timeout_ms` | Runtime stop deadline during cancellation or stop |
| `deployments.cleanup_poll_ms` | Poll interval while cleanup remains pending |
| `deployments.cleanup_timeout_ms` | Bound before cleanup becomes operator-visible failure |
| `registry.request_timeout_ms` | Timeout for one registry request |
| `registry.insecure_hosts` | Explicit development-only HTTP registry allowlist |
| `edge.entrypoint_address` | Address rendered into the complete traffic snapshot |
| `edge.management_address` | Loopback-only Gateway management address rendered into the snapshot |
| `edge.management_path_prefix` | Closed management API path rendered into the snapshot |
| `edge.management_auth_token_env` | Gateway-side environment variable that carries the management token |
| `edge.domain_verification_timeout_ms` | Bound for one production system-DNS TXT ownership lookup; 1 through 60,000 milliseconds |
| `edge.certificate_directory` | Absolute node path rendered for managed Gateway certificate files |
| `edge.certificate_ttl_ms` | Validity requested for a managed Gateway certificate |
| `edge.certificate_renewal_window_ms` | Window reserved for replacing a certificate before expiry |
| `edge.certificate_reconciliation_interval_ms` | Worker interval for renewal, revoked-claim convergence, pending command redispatch, and obsolete provider revocation; it must not exceed the renewal window |
| `edge.upstream_request_timeout_ms` | Per-upstream request timeout rendered into every route service |
| `edge.command_ttl_ms` | Independent lifetime of one complete Gateway publication command |
| `gateway.certificate_directory` | Absolute node-local root where generated Gateway keys, CSRs, and chains are stored |
| `gateway.connect_timeout_ms` | Connection timeout for the node-local Gateway management API |
| `gateway.validation_timeout_ms` | Independent deadline for validating one complete snapshot |
| `gateway.reload_timeout_ms` | Independent deadline for transactionally reloading one snapshot |
| `logs.storage_provider` | Typed log-object adapter: `local` for development or `s3`; production requires `s3` |
| `logs.s3_endpoint` | Optional absolute custom S3-compatible endpoint; empty selects the regional AWS endpoint, HTTPS is the default, and HTTP requires the development-only opt-in |
| `logs.s3_region` | Region used for S3 endpoint selection and request signing |
| `logs.s3_bucket` | Lowercase alphanumeric-and-hyphen bucket name, between 3 and 63 characters |
| `logs.s3_prefix` | Nonempty bounded object-key prefix composed of safe path segments |
| `logs.s3_access_key_env` | Name of the environment variable carrying the S3 access key ID |
| `logs.s3_secret_key_env` | Name of the environment variable carrying the S3 secret access key |
| `logs.s3_session_token_env` | Optional environment-variable name for a temporary S3 session token; empty disables it |
| `logs.s3_allow_http` | Development-only opt-in for an `http` custom endpoint; forbidden by the production profile |
| `logs.s3_virtual_hosted_style` | Whether S3 requests address the bucket as a virtual host instead of a path segment |
| `logs.s3_request_timeout_ms` | Timeout for one S3 request; 1 through 300,000 milliseconds |
| `logs.s3_connect_timeout_ms` | S3 connection timeout; 1 through 60,000 milliseconds and no longer than the request timeout |
| `logs.s3_retry_timeout_ms` | Overall S3 retry bound; at least the request timeout and at most 300,000 milliseconds |
| `logs.s3_max_retries` | Maximum S3 retries after the initial request; 0 through 10 |
| `logs.retention_ms` | Control-plane age from durable receipt before a log object becomes eligible for deletion; 1 minute through 10 years |
| `logs.retention_poll_ms` | Control-plane retention scan interval; no longer than the retention age or 24 hours |
| `logs.retention_batch_size` | Maximum metadata rows inspected by one control-plane retention scan; 1 through 10,000 |
| `logs.tombstone_retention_ms` | Age from durable `retained_at` before an individual log tombstone becomes eligible for range compaction; 1 minute through 10 years |
| `logs.tombstone_compaction_poll_ms` | Independent tombstone-compaction interval; no longer than the tombstone retention age or 24 hours |
| `logs.tombstone_compaction_batch_size` | Maximum tombstones replaced in one atomic compaction transaction; 1 through 10,000 |
| `security.certificate_authority` | Node identity PKI provider: `local` or `vault`; production requires `vault` |
| `security.gateway_certificate_authority` | Independent Gateway server-certificate provider: `local` or `vault`; production requires `vault` |
| `security.key_encryption` | Secret encryption provider: `local` or Vault Transit; production requires `vault` |
| `security.vault_address_env` | Environment-variable name carrying the absolute HTTPS Vault origin |
| `security.vault_token_env` | Environment-variable name carrying the Vault token; never an ACL credential value |
| `security.vault_pki_mount` / `security.vault_pki_role` | Vault PKI mount and role dedicated to node identities |
| `security.vault_gateway_pki_mount` / `security.vault_gateway_pki_role` | Separate Vault PKI mount and server-only role for Gateway CSRs |
| `security.vault_transit_mount` / `security.vault_transit_key` | Vault Transit mount and key for Secret encryption |
| `security.vault_timeout_ms` | Shared bounded Vault request timeout; 1 through 60,000 milliseconds |
| `logs.poll_interval_ms` | Independent node-agent interval for polling active Runtime log targets |
| `logs.max_batch_chunks` | Maximum chunk and provider-gap records in one durable upload batch; closed at 256 |
| `logs.max_batch_bytes` | Maximum log-data bytes in one durable upload batch; closed at 16 MiB |
| `docker.secret_memory_dir` | Linux tmpfs root used only for transient Docker file-Secret bind mounts |

These timers do not consume one shared request budget. API acceptance commits
desired state first; Flow, node command, Runtime, health, and cleanup deadlines
then advance independently. A mutable image tag is resolved before scheduling
and the resulting workload revision remains digest-addressable on replay.

Workload templates bind immutable Secret versions without accepting inline
material. Each binding names an exact `secretId` and positive `version`, then
selects an environment variable, an absolute file, or the workload artifact's
registry credential:

```json
{
  "name": "database-url",
  "secretId": "01900000-0000-7000-8000-000000000001",
  "version": 2,
  "target": {
    "kind": "environment",
    "variable": "DATABASE_URL"
  }
}
```

File targets use `{"kind":"file","path":"/run/secrets/key","mode":256}`.
The Linux node-agent rejects file materialization unless
`docker.secret_memory_dir` is backed by tmpfs.

Registry targets use `{"kind":"registry_credential"}`. Their referenced Secret
value is a closed, versioned JSON document:

```json
{
  "schema": "a3s.cloud.registry-credential.v1",
  "username": "registry-user",
  "password": "registry-password-or-token"
}
```

The registry address is derived from the artifact URI. During authoritative
artifact resolution, the control plane first requests the manifest
anonymously. On a Basic or Bearer authentication challenge, it reloads the
exact bound active Secret version, revalidates its tenant and environment
scope, decrypts it only in memory, and authenticates the manifest request. The
resolved revision persists only the digest and Secret reference. The node
independently resolves the same reference only when Docker must pull the
missing digest, passes it as registry authentication, and never injects it into
the workload container.

Rotating a Secret first commits its encrypted immutable version and
`secret.version.created` outbox fact. Only a worker process can consume that
durable fact. For each active running workload that still binds an older
version, it preserves the resolved artifact digest and all unrelated template
content, advances every matching binding in a new immutable revision, and
atomically commits the revision, deployment operation, causal outbox event,
and restart checkpoint. Existing nonterminal deployments defer the restart;
newer rotations supersede unstarted older ones. PostgreSQL advisory locking,
per-event/workload uniqueness, and a terminal reconciliation checkpoint make
process restart and concurrent-worker replay idempotent. No Secret material is
read by this path.

### Query workload logs

The authenticated workload log query reads one immutable revision and returns
records ordered after an opaque versioned cursor:

```text
GET /api/v1/organizations/{organizationId}/workloads/{workloadId}/revisions/{revisionId}/logs?cursor=v1:42&limit=100&stream=stdout
```

`limit` is between 1 and 256, and `stream` may be `stdout` or `stderr`. Omitting
`cursor` includes sequence zero; `cursor=v1:0` means strictly after sequence
zero. A data record carries the provider cursor, sequence, observation time,
stream, and text. If PostgreSQL metadata points to a deleted or invalid
filesystem or S3-compatible object, the same ordered position is returned as a
`gap` with reason `missing` or `corrupt`. Once the configured retention worker
deletes an expired body, its durable metadata remains at the same position as a
`retained` gap and the query does not read object storage for that row.

When Runtime proves that a provider cursor was lost or that a durable unit's
log source disappeared, the node first persists and uploads an ordered gap with
reason `provider_cursor_lost` or `provider_disconnected`. After the exact
receipt, it clears only the provider cursor, retains the Cloud sequence
watermark, resumes from the earliest available provider record, and rebases
replacement chunk sequences monotonically. Provider gaps have no known stream,
so they remain visible under `stdout` or `stderr` filtering; their source cursor
is nullable.

After the separate tombstone retention age, a bounded worker atomically replaces
eligible per-chunk tombstones with coalesced sequence ranges. Those ranges are
returned as `gapReason: "compacted"` with `fromSequence`, `throughSequence`, and
`compactedChunks`; `sourceCursor`, `observedAtMs`, and `stream` are `null`, and
`sequence` is the terminal range position used for paging. A stream-filtered
query still includes compacted ranges because per-chunk stream metadata has
been discarded. Durable batch headers and sequence watermarks remain, so an
exact old-batch replay returns its receipt without recreating objects and an
unseen sequence must advance beyond all live, provider-gap, or compacted
history. Storage unavailability and retryable Runtime/provider transport
failure remain errors, not fabricated gaps.

The live endpoint reuses the same authorization and record shape:

```text
GET /api/v1/organizations/{organizationId}/workloads/{workloadId}/revisions/{revisionId}/logs/stream?cursor=v1:42&limit=16&stream=stdout
```

It emits `records` SSE events whose `id` and `nextCursor` are the terminal
`v1:<sequence>` included in that event. `Last-Event-ID` takes precedence on
reconnect. Each poll reads at most 16 records, each encoded JSON event is at
most 8 MiB, idle streams send keepalives, and storage or query failure closes
the stream for bounded client retry. The web console retains at most the latest
500 deduplicated records and keeps provider and compaction gaps visible under
stdout/stderr filtering.

See [`config/cloud.acl`](config/cloud.acl) and
[`config/node.example.acl`](config/node.example.acl) for the complete control
plane and node-agent profiles.

## Platform Model

### Tenancy

```text
Organization
└── Project
    └── Environment
        ├── desired workload revisions
        ├── deployments and operations
        └── routes, data services, and persistent storage
```

Bearer authentication is global except for bootstrap and health routes. A token
is bound to an organization unless it carries the platform administrator role,
and command handlers enforce both tenant ownership and the required scope.

### Asset catalog

Cloud publishes immutable versions of three A3S asset kinds:

| Asset | Immutable artifact | Runtime role |
| --- | --- | --- |
| Agent | Validated manifest and digest-pinned OCI artifact | Finite Task or long-running Service |
| MCP | Validated manifest and digest-pinned OCI artifact | Long-running MCP Service |
| Skill | Content-addressed bundle and validated manifest | Immutable input bound to an Agent revision |

Each published version records source and artifact provenance. Agent and MCP
versions enter the same workload revision and durable deployment pipeline used
by applications. Skill versions are bound as immutable workload inputs.

### Runtime boundary

A3S Runtime remains provider-neutral and exposes two lifecycle classes:

| Class | Purpose |
| --- | --- |
| Task | Finite work such as a build, migration, evaluation, or backup |
| Service | Long-running work such as an application, Agent, or MCP server |

Runtime owns capability discovery and idempotent `apply`, `inspect`, `stop`, and
`remove` operations. Cloud owns scheduling policy, deployment workflows,
routing, release provenance, and convergence decisions.

## Architecture

A3S Cloud starts as a modular monolith with a separate outbound-only node agent.
API, worker, and event-relay roles can run in one process or independently from
the same binary.

```text
Browser
   │
   v
A3S Boot API ───> DDD application modules ───> PostgreSQL
   │                       │                         │
   │                       ├──> A3S Flow <───────────┤
   │                       └──> transactional outbox │
   │                                      │          │
   │                                      v          │
   │                                  A3S Event      │
   │                                                 │
   │       outbound mTLS command lease               │
   v                                                 │
Node agent ───> A3S Runtime ───> Docker / containerd / A3S Box
   │
   ├──────────> A3S Gateway ───> active edge revision
   └──────────> observations and durable acknowledgements
```

### Ownership and recovery

| Concern | Authority |
| --- | --- |
| Tenant, project, environment, and desired workload state | PostgreSQL domain tables |
| Long-running operation history | A3S Flow PostgreSQL event store |
| Provider resources and live health | Node agent and A3S Runtime provider |
| Active edge configuration | A3S Gateway configuration revision |
| Integration-fact delivery | Transactional outbox and A3S Event |
| OCI images and immutable bundles | OCI registry or S3-compatible object storage |

PostgreSQL is the desired-state authority. A3S Flow owns durable operation
progress. Event delivery accelerates coordination but is never the only recovery
path. Reconcilers compare desired and observed generations until success is
proven or a terminal failure is recorded.

### A3S components

| Component | Responsibility |
| --- | --- |
| A3S Boot | Modular API, dependency injection, CQRS, authentication, health, and OpenAPI |
| A3S ORM | Typed PostgreSQL access, transactions, and migrations |
| A3S Runtime | Provider-neutral Task and Service lifecycle |
| A3S Flow | Durable operations, retries, timers, and worker leases |
| A3S Event | Integration-fact delivery through local or NATS providers |
| A3S Gateway | HTTPS, ACME, routing, health, and atomic configuration reload |
| A3S ACL | Closed product configuration and validated manifests |

Business modules follow four DDD layers. Domain code remains independent of
A3S Boot, SQL, HTTP, Runtime, Flow, Event, and provider SDKs; infrastructure
adapters implement typed ports owned by the inner layers.

See [Technical Architecture](docs/architecture.md) for the node protocol,
security model, consistency boundaries, and failure recovery.

## Delivery Roadmap

| Gate | Outcome | State |
| --- | --- | --- |
| R0 — Universal Runtime | General Task and Service contracts, durable identity, capability matching, and Docker conformance | Verified |
| F0 — Foundation | Boot control plane, PostgreSQL, identity, tenancy, Flow operations, outbox, projections, and web shell | Verified |
| N0 — Node control | Enrollment, mTLS, command leases, observations, command journal, and Docker driver | Verified |
| D0 — OCI deployment | Immutable workload revisions, one-node scheduling, apply, health, activation, stop, cancellation, and recovery | Verified |
| E0 — Reachable service | Edge desired state, managed TLS, encrypted Secret injection and rotation recovery, durable ordered logs, one-node immutable update, activation-before-retirement process-death recovery, cloned rollback, authoritative Web operations, and the exact clean-host Linux release loop through A3S Gateway 1.0.12 and one outbound Docker node | Verified |
| G0 — External source delivery | Pinned Git commits, isolated builds, OCI publication, provenance, and deployment through the existing workload path | Planned |
| P0 — Developer workflows | Detected build plans, web/worker/scheduled profiles, pull-request previews, monorepo affected sets, and closed Compose import | Planned |
| C0 — Control surfaces | REST/CLI/MCP parity, team grants, notifications, audit, and outbound-protocol exec/terminal | Planned |
| A0 — Release catalog | Agent and MCP release import, Skill bundle publication, and deployment through the common path | Planned |
| S0 — Stateful platform | Explicit databases and volumes with fencing, backup, restore, and retention | Planned |
| H0 — Production scale | Durable replicas, multi-node placement, Gateway replication, HA, and measured autoscaling | Planned |

E0 is the first verified usable release: one control plane, one Linux node,
Docker-backed stateless workloads, and a repeatable end-to-end deployment on a
clean host. Its release evidence includes crash injection at each durable
boundary, recovery without duplicate provider resources, A→B→cloned-A routed
cutover, durable stop, and exact cleanup back to the host baseline.

With E0 verified, G0 source delivery, C0 control surfaces, and S0 stateful
foundations may advance as independent lanes. P0 builds on G0, A0 reuses the
same build and deployment path, and H0 scales only the single-node semantics
proven by the earlier gates.

Cloud intentionally does not own a built-in mail server, a separate native
desktop feature set, or commercial billing. A3S Gateway owns edge transport,
TLS, compression, and cache mechanics; Cloud owns versioned desired policy and
exact applied-state projection.

See the [Development Plan](docs/development-plan.md) for milestone order and
acceptance criteria.

## Repository

Cloud is an app-local Rust workspace inside the A3S monorepo:

```text
Cloud/
├── Cargo.toml
├── config/
│   ├── cloud.acl
│   └── node.example.acl
├── deploy/                 # local infrastructure profiles
├── migrations/
├── crates/
│   ├── contracts/          # versioned public and node protocol types
│   ├── control-plane/      # API, worker, reconciler, and event relay
│   └── node-agent/         # outbound node process and Runtime adapters
├── web/                    # React control-plane console
└── docs/
```

## Development

Run Rust checks from the Cloud repository directory:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

### Certify the Runtime boundary

Run real-provider certification only on a dedicated Linux or A3S OS runner.
Prepare clean `apps/cloud` and `crates/runtime` worktrees directly through Git
at the exact 40-character commits. Cloud pins its compatible Runtime commit in
`tools/runtime-conformance/runtime-revision`. Then run the isolated gate from
the Cloud worktree. The default suite certifies the Docker provider without
restarting or reconfiguring the host Docker daemon:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA"
```

After the provider suite passes, run the Cloud consumer recovery gate with its
pinned PostgreSQL and NATS services:

```bash
sudo tools/runtime-conformance/run_isolated_docker_gate.sh \
  --source-root /var/tmp/a3s-runtime-tests/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --suite cloud
```

The provider suite covers Base, Recovery, Networking, Mounts, Health,
Resources, Logs, and Security, including continuity of an exact pre-restart log
cursor across isolated Docker daemon replacement. The Cloud suite covers
persisted projections, the command journal, restart, JetStream redelivery,
reconciliation, real
PostgreSQL-backed Secret authorization, Docker injection, redacted log
persistence, a real child-process death after immutable object publication but
before PostgreSQL receipt, exact orphan adoption, ordered REST corruption
projection, cancellation, failed-update preservation, cleanup, manual
rollback, and Secret-rotation restart recovery after the committed version
boundary. The real Docker update-and-rollback case deploys healthy A, proves an
unhealthy B cannot replace it, activates a distinct healthy C, stops A only
after C is selected, clones A into a new generation, and stops C only after the
rollback is selected. The PostgreSQL recovery case blocks retirement dispatch,
lets a child Flow process durably select the rotated revision, proves no stop
command committed, sends `SIGKILL`, then reconstructs the coordinator and
requires one deterministic previous-revision stop before terminal activation.
The rotation restart case races reconstructed workers,
derives one new revision with the pinned artifact unchanged, reconstructs Flow
after the reference-only Runtime result, and scans the restart/checkpoint,
desired-state, Flow, Fleet, event, audit, log, digest, and API surfaces for
plaintext. During the real rotated apply, a child pauses after the healthy
Docker resource exists while its Runtime receipt is still pending. The parent
restarts the labeled isolated provider, verifies the exact container survives,
kills the child agent, reconstructs Runtime, reattaches that container,
completes and replays the exact receipt, validates `0400` material and redacted
logs, and removes the resource and tmpfs material. Its
Secret file root is a run-specific tmpfs directory and must be empty after the
test. The dedicated Linux Secret/log CI job additionally
provisions an authenticated private registry, removes the cached workload
image, and certifies both production control-plane manifest resolution and the
node registry-credential pull path. Both suites require zero provider and host
inventory drift. See
[`tools/runtime-conformance/README.md`](tools/runtime-conformance/README.md) for
the pinned images, safety model, and evidence contract.

The PostgreSQL integration test treats the supplied URL as an administration
connection, creates a uniquely named database for the run, and force-removes it
after success, ordinary failure, or assertion panic. It never migrates or
truncates the supplied development database. Add NATS and Docker to exercise
JetStream redelivery and the real D0 Runtime path in the same run:

```bash
A3S_CLOUD_TEST_POSTGRES_URL="postgres://a3s_cloud:a3s_cloud@127.0.0.1:54320/a3s_cloud" \
A3S_CLOUD_TEST_NATS_URL="nats://127.0.0.1:42220" \
A3S_CLOUD_TEST_DOCKER=1 \
cargo test -p a3s-cloud-control-plane --test postgres_integration -- --nocapture
```

Run the remaining real-provider gates explicitly:

```bash
A3S_CLOUD_TEST_DOCKER=1 \
cargo test -p a3s-cloud-node-agent \
  --test docker_conformance \
  real_docker_passes_all_advertised_runtime_profiles \
  -- --ignored --exact --nocapture --test-threads=1

A3S_CLOUD_TEST_DOCKER=1 \
cargo test -p a3s-cloud-control-plane --test docker_deployment -- --nocapture

A3S_CLOUD_TEST_REGISTRY_URL="http://127.0.0.1:50020/" \
cargo test -p a3s-cloud-control-plane --test oci_registry_integration -- --nocapture

A3S_CLOUD_TEST_GATEWAY_BIN="$(command -v a3s-gateway)" \
cargo test -p a3s-cloud-node-agent --lib \
  gateway::remote_tests::installed_a3s_gateway_validates_and_reloads_complete_snapshots \
  -- --ignored --exact --nocapture --test-threads=1

A3S_CLOUD_TEST_GATEWAY_BIN="$(command -v a3s-gateway)" \
cargo test -p a3s-cloud-control-plane \
  installed_gateway_validates_compiled_snapshot -- --nocapture

A3S_CLOUD_TEST_GATEWAY_BIN="$(command -v a3s-gateway)" \
cargo test -p a3s-cloud-node-agent --lib \
  gateway::remote_tests::installed_a3s_gateway_serves_managed_tls_after_exact_snapshot_reload \
  -- --ignored --exact --nocapture --test-threads=1

A3S_CLOUD_TEST_GATEWAY_BIN="$(command -v a3s-gateway)" \
cargo test -p a3s-cloud-node-agent --lib \
  gateway::reload_crash_tests::installed_a3s_gateway_recovers_reload_after_agent_process_death \
  -- --ignored --exact --nocapture --test-threads=1

A3S_CLOUD_TEST_S3_ENDPOINT="http://127.0.0.1:9000" \
A3S_CLOUD_TEST_S3_REGION="us-east-1" \
A3S_CLOUD_TEST_S3_BUCKET="a3s-cloud-disposable-test" \
A3S_CLOUD_TEST_S3_ACCESS_KEY_ID="test-access-key" \
A3S_CLOUD_TEST_S3_SECRET_ACCESS_KEY="test-secret-key" \
cargo test -p a3s-cloud-control-plane --lib --locked \
  modules::fleet::infrastructure::s3_log_chunk_store::tests::real_s3_compatible_store_preserves_immutable_log_semantics \
  -- --ignored --exact --nocapture --test-threads=1
```

The first Gateway command verifies route-less snapshot transport and node-local
CAS. The second is the real route-bearing compiler gate. A3S Gateway 1.0.12
fixes the ACL recursion defect present in 1.0.11, and the generated
router/service snapshot passes that gate. The third command generates a private
key and CSR on the node, provisions the managed certificate, reloads the exact
HTTPS snapshot, trusts the fixture CA, and reaches a loopback upstream through
DNS/SNI. The fourth command durably begins the Gateway command, reloads the real
process, forces `SIGKILL` before installer-state or acknowledgement completion,
then reconstructs the agent. Redelivery repeats one idempotent reload, persists
the exact installed revision and applied acknowledgement, and a second restart
replays that outcome without another reload. Both real Gateway paths run in the
dedicated CI job.

The production profile performs bounded ownership verification through the
system DNS resolver and uses a dedicated Vault PKI mount/role to sign
node-generated Gateway CSRs and revoke the resulting provider serial. The
worker redispatches pending certificate commands, renews within the configured
window, filters routes whose claims were explicitly revoked, and applies
route/certificate changes only for the exact Gateway acknowledgement. Rejected
replacement snapshots leave the previous certificate and active routes
authoritative; successful convergence retries provider revocation until each
unreferenced old serial is durably marked revoked.

The final S3 command must target a disposable bucket controlled by the test
operator. The dedicated CI job creates a fresh bucket in digest-pinned MinIO,
exercises conditional create, exact replay, verified read, deliberate object
corruption, immutable repair rejection, idempotent delete, and readiness
cleanup, then removes the provider container. No credential value is stored in
ACL configuration.

### Certify the clean-host E0 release

The final E0 gate requires exact clean Cloud and pinned Runtime worktrees on a
dedicated Linux Docker host, plus A3S Gateway 1.0.12:

```bash
runtime_revision=$(<tools/runtime-conformance/runtime-revision)
tools/release-conformance/run_clean_host_gate.sh \
  --source-root /var/tmp/a3s-cloud-release/release-candidate \
  --cloud-sha "$CLOUD_SHA" \
  --runtime-sha "$runtime_revision" \
  --gateway "$(command -v a3s-gateway)"
```

A passing run emits `A3S_CLOUD_CLEAN_HOST_E0_PASS` only after the A→B→cloned-A
TLS route, ordered and cursor-resumed log evidence, three distinct provider
resources, durable stop, clean source trees, exact container/volume/network
inventory restoration, and an empty generated-credential scan all pass. See
[`tools/release-conformance/README.md`](tools/release-conformance/README.md)
for host preparation and the complete evidence contract.

Run web checks from `web/`:

```bash
bun install --frozen-lockfile
bun run typecheck
bun run format:check
bun run lint:check
bun run test
bun run build
```

Design references:

- [Domain Model](docs/domain-model.md)
- [Technical Architecture](docs/architecture.md)
- [Development Plan](docs/development-plan.md)

## License

MIT
