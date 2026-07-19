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
  bind certificate intent into the complete snapshot digest, issue only public
  certificate material over node mTLS, and keep generated private keys on the
  Gateway node
- **Encrypted Secret Resources**: Create tenant-scoped Secret identities,
  rotate immutable encrypted versions through local AES-GCM or Vault Transit,
  bind exact versions to workload environment/file targets, materialize only
  for the assigned node over mTLS, and inject at the Docker boundary without
  placing plaintext in desired state, Runtime state, commands, or events
- **Runtime Observations**: Record provider capabilities, workload state,
  health, logs, and durable command acknowledgements from A3S Runtime
- **Durable Workload Logs**: Project active Runtime targets from the command
  journal, persist one bounded batch before mTLS upload, resume only after an
  exact receipt, redact bound Secret values at the Docker log boundary, and
  query verified local chunk objects through tenant-scoped cursor pages while
  a configurable worker deletes expired bodies and preserves explicit missing,
  corrupt, or retained positions
- **Digest-Pinned Deployments**: Resolve mutable OCI tags once, persist the
  resulting digest, schedule one eligible node, and activate only after real
  Runtime health evidence
- **Convergent Recovery**: Reattach after provider creation, recover a lost
  provider at the same generation, preserve the prior healthy revision on a
  failed update, and drive cancellation through bounded cleanup
- **Operation Streaming**: Expose tenant-scoped snapshots and resumable
  server-sent events with stable content-derived event identifiers
- **Web Console**: Sign in with a session-scoped API token, select the active
  organization, project, and environment, and inspect desired revisions,
  observed Runtime state, health, cancellation, and live operation progress

### Delivery capability matrix

| Area | Capability | State |
| --- | --- | --- |
| Runtime prerequisite | General Task and Service lifecycle with provider capability matching | Complete |
| Foundation | Identity, tenancy, PostgreSQL, Flow, outbox, projections, API, and web shell | Complete |
| Node control | Enrollment, node identity, outbound mTLS, command leases, and observations | Complete |
| Deployment | Digest-pinned OCI revisions, scheduling, apply, health, activation, stop, cancellation, and recovery | Complete |
| Reachability | Route ownership, managed TLS policy and provisioning, routed Gateway validation, complete snapshot publication, and exact acknowledgement projection are implemented; production DNS/CA providers, renewal, update, rollback, and crash recovery remain | In progress (`E0`) |
| Secrets | Encrypted tenant-scoped resources, immutable rotation/revocation, typed workload bindings, assigned-node mTLS materialization, Docker environment/file injection, metadata-only APIs/events, and reference-only durable state are implemented; real-provider restart/crash gates and full redaction scans remain | In progress (`E0`) |
| Logs | Restart-safe bounded node shipping, Docker-bound Secret redaction, PostgreSQL metadata, verified local chunk objects, cursor paging, tenant isolation, configurable local body retention, and explicit missing/corrupt/retained gaps are implemented; production S3-compatible storage, tombstone compaction, provider cursor-loss recovery, real crash certification, and live web logs remain | In progress (`E0`) |
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
| `edge.certificate_directory` | Absolute node path rendered for managed Gateway certificate files |
| `edge.certificate_ttl_ms` | Validity requested for a managed Gateway certificate |
| `edge.certificate_renewal_window_ms` | Window reserved for replacing a certificate before expiry |
| `edge.upstream_request_timeout_ms` | Per-upstream request timeout rendered into every route service |
| `edge.command_ttl_ms` | Independent lifetime of one complete Gateway publication command |
| `gateway.certificate_directory` | Absolute node-local root where generated Gateway keys, CSRs, and chains are stored |
| `gateway.connect_timeout_ms` | Connection timeout for the node-local Gateway management API |
| `gateway.validation_timeout_ms` | Independent deadline for validating one complete snapshot |
| `gateway.reload_timeout_ms` | Independent deadline for transactionally reloading one snapshot |
| `logs.retention_ms` | Control-plane age from durable receipt before a log object becomes eligible for deletion; 1 minute through 10 years |
| `logs.retention_poll_ms` | Control-plane retention scan interval; no longer than the retention age or 24 hours |
| `logs.retention_batch_size` | Maximum metadata rows inspected by one control-plane retention scan; 1 through 10,000 |
| `logs.poll_interval_ms` | Independent node-agent interval for polling active Runtime log targets |
| `logs.max_batch_chunks` | Maximum chunks in one durable upload batch; closed at 256 |
| `logs.max_batch_bytes` | Maximum log-data bytes in one durable upload batch; closed at 16 MiB |
| `docker.secret_memory_dir` | Linux tmpfs root used only for transient Docker file-Secret bind mounts |

These timers do not consume one shared request budget. API acceptance commits
desired state first; Flow, node command, Runtime, health, and cleanup deadlines
then advance independently. A mutable image tag is resolved before scheduling
and the resulting workload revision remains digest-addressable on replay.

Workload templates bind immutable Secret versions without accepting inline
material. Each binding names an exact `secretId` and positive `version`, then
selects either an environment variable or an absolute file target:

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

### Query workload logs

The authenticated workload log query reads one immutable revision and returns
records ordered after an opaque versioned cursor:

```text
GET /api/v1/organizations/{organizationId}/workloads/{workloadId}/revisions/{revisionId}/logs?cursor=v1:42&limit=100&stream=stdout
```

`limit` is between 1 and 256, and `stream` may be `stdout` or `stderr`. Omitting
`cursor` includes sequence zero; `cursor=v1:0` means strictly after sequence
zero. A data record carries the provider cursor, sequence, observation time,
stream, and text. If PostgreSQL metadata points to a deleted or invalid local
object, the same ordered position is returned as a `gap` with reason `missing`
or `corrupt`. Once the configured local retention worker deletes an expired
body, its durable metadata remains at the same position as a `retained` gap and
the query does not read object storage for that row. Storage unavailability
remains an API error. The endpoint is snapshot paging, not the still-planned
live web stream.

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
| E0 — Reachable service | Edge desired state, managed TLS mechanics, exact activation projection, encrypted Secret injection, and the restart-safe local workload-log path with body retention are implemented; production certificate automation, Secret/log real-provider crash acceptance, S3 log storage, tombstone compaction, update, rollback, and web timeline remain | In progress |
| G0 — External source delivery | Pinned Git commits, isolated builds, OCI publication, provenance, and deployment through the existing workload path | Planned |
| P0 — Developer workflows | Detected build plans, web/worker/scheduled profiles, pull-request previews, monorepo affected sets, and closed Compose import | Planned |
| C0 — Control surfaces | REST/CLI/MCP parity, team grants, notifications, audit, and outbound-protocol exec/terminal | Planned |
| A0 — Release catalog | Agent and MCP release import, Skill bundle publication, and deployment through the common path | Planned |
| S0 — Stateful platform | Explicit databases and volumes with fencing, backup, restore, and retention | Planned |
| H0 — Production scale | Durable replicas, multi-node placement, Gateway replication, HA, and measured autoscaling | Planned |

The first usable release remains E0: one control plane, one Linux node,
Docker-backed stateless workloads, and a repeatable end-to-end deployment on a
clean host. Its exit gate includes crash injection at each durable boundary,
recovery without duplicate provider resources, and rollback to the previous
healthy revision.

After E0, G0 source delivery, C0 control surfaces, and S0 stateful foundations
may advance as independent lanes. P0 builds on G0, A0 reuses the same build and
deployment path, and H0 scales only the single-node semantics proven by the
earlier gates.

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
Resources, Logs, and Security. The Cloud suite covers persisted projections,
the command journal, restart, JetStream redelivery, reconciliation, log
transport, cancellation, failed-update preservation, and cleanup. Both suites
require zero provider and host inventory drift. See
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
```

The first Gateway command verifies route-less snapshot transport and node-local
CAS. The second is the real route-bearing compiler gate. A3S Gateway 1.0.12
fixes the ACL recursion defect present in 1.0.11, and the generated
router/service snapshot passes that gate. The final Gateway command is also a
dedicated CI job: it generates a private key and CSR on the node, provisions the
managed certificate, reloads the exact HTTPS snapshot, trusts the fixture CA,
and reaches a loopback upstream through DNS/SNI. Production DNS and certificate
authority providers plus automated renewal remain E0 work.

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
