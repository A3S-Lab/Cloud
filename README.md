# A3S Cloud

<p align="center">
  <strong>Self-Hosted Desired-State Control Plane for A3S</strong>
</p>

<p align="center">
  <em>Deploy immutable workloads, converge infrastructure, and operate services on systems you own</em>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#platform-model">Platform Model</a> •
  <a href="#delivery-model">Delivery Model</a> •
  <a href="#gateway-relationship">Gateway Relationship</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Cloud** is a self-hosted control plane that stores desired state in
PostgreSQL and converges it through durable operations. Organizations, projects,
and environments define the tenancy boundary. Outbound node agents execute
provider-neutral A3S Runtime commands, apply identity-bound A3S Gateway
snapshots, and report durable observations to the control plane.

Cloud accepts intent rather than holding an HTTP request open for deployment
work. A mutation commits desired state and an operation identity, then A3S Flow,
reconcilers, and node command leases continue the work across retries and
process restarts.

Cloud is not a reverse proxy, an inference byte path, or a replacement Runtime.
It owns business state, scheduling and deployment policy, rollout and
autoscaling decisions, complete Gateway policy, operations, and management
surfaces. Runtime owns provider lifecycle mechanics, while Gateway owns
transport and request-path enforcement.

### Basic usage

From the Cloud repository directory:

```bash
just cloud
```

This starts the pinned development dependencies, control-plane API, and
hot-reloading web console. The API listens on `127.0.0.1:8080` by default.

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
```

## Features

- **Tenant Model**: Isolate organizations, projects, environments, resources,
  commands, queries, and observations
- **Scoped Identity**: Bootstrap the first organization, issue expiring scoped
  API tokens, and revoke credentials without storing token plaintext
- **Durable Operations**: Persist intent before execution and resume A3S Flow
  operations, leases, retries, cleanup, and projections after interruption
- **Outbound Node Control**: Enroll Linux nodes, rotate mTLS identities, lease
  idempotent commands, and receive observations without opening inbound node
  management ports
- **Versioned Node Inventory**: Detect real host CPU and state-filesystem
  capacity, report Linux memory when available, preserve content-addressed
  generations across Agent restarts, and bind v2 heartbeats to the current
  Fleet snapshot
- **Immutable Workloads**: Resolve OCI images to digests, create versioned
  workload revisions, schedule an eligible node, and activate only after
  Runtime health evidence
- **Managed Replica Foundation**: Persist an inference-neutral owner,
  effective placement policy, stable replica/member identity, exact deployment
  binding, current-inventory requirement compilation, shared scalar capacity,
  and fenced hard-resource claims without exposing Cloud placement identity to
  Runtime
- **Managed Reachability**: Create tenant-scoped logical Gateway scopes, persist
  ordered physical membership and explicit rollout thresholds, verify domain
  ownership, provision TLS, compile complete expiring ACL snapshots from
  command-bound healthy Runtime targets, renew unchanged policy without
  reissuing TLS, and advance routes only after Gateway acknowledges the exact
  identity, revision, digest, validity, and readiness
- **Encrypted Secrets**: Store tenant-scoped immutable Secret versions and
  materialize exact bindings only at authenticated registry or assigned-node
  boundaries
- **Durable Logs**: Ship bounded ordered Runtime logs, preserve explicit gaps,
  redact bound Secrets, store immutable chunks, and expose cursor and resumable
  SSE queries
- **Safe Changes**: Replace an active workload immutably, preserve the prior
  healthy revision until cutover, and roll back by cloning a proven template
  into a new generation
- **Source Delivery Slices**: Resolve exact GitHub commits, run isolated
  BuildKit tasks, validate trusted content-addressed caches and complete OCI
  graphs, publish by digest, freeze locally verified signed SPDX/SLSA evidence,
  and hand successful builds to the existing workload deployment path
- **Web Operations**: Inspect deployment history, route and certificate state,
  Runtime health, logs, BuildRuns, updates, rollback, cancellation, and retry

### Delivery capability matrix

| Gate | Product outcome | State |
| --- | --- | --- |
| `R0` — Universal Runtime | Task and Service contracts, durable identity, capability matching, and Docker conformance | Verified |
| `F0` — Foundation | A3S Boot API, PostgreSQL, tenancy, identity, Flow operations, outbox, projections, and web shell | Verified |
| `N0` — Node control | Enrollment, outbound mTLS, command leases, observations, command journal, and Docker driver | Verified |
| `D0` — OCI deployment | Digest-pinned revisions, one-node scheduling, apply, health, activation, stop, cancellation, and recovery | Verified |
| `E0` — Reachable service | Managed TLS, complete Gateway snapshots, encrypted Secrets, ordered logs, immutable update, cloned rollback, web operations, and clean-host recovery | Verified |
| `G0` — External source delivery | Pinned Git sources, isolated builds, trusted retry caches, OCI validation and publication, signed SPDX/SLSA evidence, and deployment handoff | In progress |
| `P0` — Developer workflows | Build detection, workload profiles, previews, monorepos, and closed Compose import | Planned |
| `C0` — Control surfaces | Stable REST, CLI, management MCP, grants, collaboration, notifications, audit, and bounded terminal access | Planned |
| `A0` — Release catalog | Immutable Agent and MCP releases plus Skill publication through the common delivery path | Planned |
| `S0` — Stateful platform | Databases, volumes, fencing, backup, restore, and retention | Planned |
| `H0` — Production scale | Replicas, multi-node placement, private networking, Gateway replication, HA, and measured autoscaling | In progress |
| `I0` — Inference profile | Accelerator-backed serving, OpenAI-compatible traffic, scoped keys, Providers, routing, usage, and governed self-service | Planned |

`R0` through `E0` are one cumulative verified baseline: one control plane, one
outbound Linux node, Docker-backed stateless workloads, managed HTTPS, ordered
logs, immutable update and rollback, and repeatable cleanup. Later gates must
reuse this deployment and reconciliation path.

`G0` already includes GitHub App connections and signed webhooks, private
repository access, immutable source revisions, BuildRuns, cancellation, retry,
ordered logs, command-bound Artifact transport, isolated BuildKit execution,
trusted content-addressed retry caches, complete OCI graph validation,
digest-only registry publication, deterministic SPDX/SLSA generation,
locally verified Ed25519 DSSE signing through persistent local or Vault Transit
providers, durable evidence restoration, evidence API/web download, replay
adoption, cleanup, and explicit deployment handoff. It remains in progress
until external private-provider certification and the production
signed-evidence process-death gate pass.

The current `H0.1` foundation maps every existing single-instance Workload to
one stable replica and member. Replica identity survives immutable revision
changes, while every deployment records the exact replica generation, member,
node, placement generation, and opaque Runtime unit identity it projects.
Managed-owner references and the effective placement policy are durable Cloud
state; managed Workloads reject direct mutations that do not carry the exact
owner and policy. Workload list/detail responses expose this control and
placement state explicitly.

Hard-resource reservations define canonical sorted slot requests and a durable
claim state machine for database reservation, Agent preparation, Runtime
binding, release, and operator-visible orphaning. CPU, memory, and ephemeral
storage are shared scalar capacities; accelerator, host-port, and volume slots
remain exclusive. PostgreSQL serializes each slot, sums active shared
allocations, rejects over-capacity reservations, and advances a monotonic slot
generation with a new unguessable fence token. An orphan or timeout continues
to block its allocation; only exact Agent release, provider NotFound, or
trusted compute-fencing evidence makes it reusable.

The deployment scheduler now compiles CPU, memory, and optional ephemeral
storage requirements from the current Fleet inventory. Its PostgreSQL
reservation transaction locks and verifies the exact inventory head, reserves
capacity before persisting placement, and uses the Deployment ID as the
deterministic Claim ID. Replay recovers a crash between reservation and node
assignment from the durable claim, while a capacity conflict on one candidate
continues to the next eligible node. PID limits remain Runtime-local because
the current inventory contract has no PID slot. Cancellation, retirement, and
stop may release a claim with database-only evidence only while it is still
`reserved_in_db`; an issued, prepared, bound, or orphaned claim still requires
Agent or trusted fencing evidence.

Migrations 040 and 041 backfill the replica foundation and add claim,
slot-evidence, and current-lease tables. Migration 043 replaces universal
active-slot uniqueness with exclusive-kind uniqueness so shared capacities can
carry multiple bounded claims. The complete Workloads PostgreSQL repository
uses A3S ORM typed tables and builders for reads, JOINs, ordering, counts,
inserts, updates, idempotency records, outbox writes, PostgreSQL row and
advisory locks, `SKIP LOCKED`, and parameterized JSONPath Secret-binding
predicates. No Workloads production persistence file uses raw SQL or a direct
database driver; an architecture test enforces that boundary. In-memory and
PostgreSQL 17 gates cover exact replay, competing exclusive and shared
reservations, over-capacity rejection, stale inventory rejection, fencing,
release, and generation/token rotation.

Fleet now persists strict `NodeResourceInventory` snapshots, their normalized
slots, and one current generation/digest head per enrolled node. The node agent
detects CPU and state-filesystem capacity, adds Linux `MemTotal` when the host
can report it, and deliberately omits accelerators, ports, volumes, memory on
unsupported hosts, and networking it cannot prove. It stores the canonical
snapshot locally, keeps its generation across restart, and advances that
generation only when slot content changes. The authenticated
`POST /v1/node-control/inventories` endpoint accepts exact replay and exact
next-generation content changes while rejecting skips, reused generations, and
identity conflicts. New v2 observation batches are accepted only when their
heartbeat references the current inventory generation and digest; legacy v1
batches remain readable during migration. Migration 042 and the inventory
persistence adapter use only typed A3S ORM tables, expressions, transactions,
locks, inserts, updates, and joins. Contract, Agent, API, in-memory, and real
PostgreSQL 17 gates cover canonical digests, restart reuse, concurrent replay,
historical replay without head regression, stale-heartbeat rejection, and
claim rejection after the inventory head advances.

This does not complete `H0.1`: Agent prepare/release commands and journal
enforcement, Runtime allocation-binding evidence, and process/provider crash
reconciliation still have to converge through the ordinary deployment
workflow before the provider-unit exit gate can pass.

`H0.2` now has Cloud-owned logical Gateway scopes plus a Gateway-native
snapshot and generation-bound private-target foundation. A scope belongs to
one organization, project, and environment. Its desired state can contain an
ordered set of physical Gateway members, a membership generation, and explicit
`minReady` and `maxUnavailable` policy; the legacy `nodeId` request remains the
single-member form. The first member is the bootstrap primary used by the
current route compiler. `POST` and `GET` on the environment's
`/gateway-scopes` resource create and list these scopes. Route publication
requires a `gatewayScopeId` whose tenancy and bootstrap primary match the
verified DomainClaim and healthy Runtime target. `Route` stores both the
logical scope and physical node; Gateway continues to receive only its
node-addressed managed snapshot and does not become the owner of Cloud tenancy.

Every route persists its immutable workload revision, deterministic Runtime
unit identity, positive generation, declared port, canonical node-local HTTP
origin, and observation time. Cloud accepts the target only from the exact
deployment command's current healthy Runtime observation. The compiler binds
revision, unit, and generation into the ACL digest, so even a reused origin at
a new generation produces a distinct snapshot.

Cloud records an applied acknowledgement only after Gateway reports the same
identity, revision, digest, expiry, applied metadata, and ready state.
Before any mutation, the node agent reads Gateway's versioned capability
descriptor and selects the exact management protocol plus request/status
schemas. It also supports the closed pre-descriptor v1 response for an older
Gateway; unknown or inconsistent tuples fail before apply. New Gateway
acknowledgements carry that selection under v4 and the enclosing command
acknowledgement is v2. The control plane still reads the prior v3/v1 pair during
the migration window.

Generation cutover requires a different immutable revision, a strictly newer
generation, and the same logical scope; rejection preserves the prior target,
while an exact applied acknowledgement atomically replaces the complete target
projection. PostgreSQL migration 036 deterministically creates one logical
scope per legacy environment/node binding, backfills Route and recovery
documents, and enforces the full tenancy/node relationship. Recreated
PostgreSQL and migration gates verify recovery and reject cross-environment or
wrong-node publication. Migration 037 stores new protocol evidence without
inventing it for legacy acknowledgements.

The replicated control-plane foundation persists one `GatewayRollout`
aggregate with an independent revision, command, digest, expiry, certificate,
and terminal result for every desired physical member. Meeting the configured
threshold makes the rollout ready to serve; only exact success from every
member makes it succeeded, while a fully observed mixed result becomes
explicitly degraded. Migrations 038 and 039 preserve legacy single-member
scopes, add membership and rollout constraints, and recover the aggregate from
PostgreSQL. The complete Edge PostgreSQL repository now uses A3S ORM typed
tables and query builders for logical scopes and membership, publications,
routes, cutovers, acknowledgements, DomainClaims, managed certificates,
certificate convergence, and replicated rollouts. Typed expressions preserve
joins, correlated `EXISTS`, scalar aggregate subqueries, `COALESCE`/`LEAST`
deadline ordering, optimistic updates, row locks, and the DomainClaim table
lock. No Edge production persistence file uses raw SQL or a direct database
driver; a source architecture test enforces that boundary. Any missing typed
primitive must be filed, implemented, and tested in A3S ORM before new
persistence behavior ships. A worker-role reconciler loads each active rollout
and its publications through one typed CTE/JOIN query, idempotently
redispatches every pending member command after process or queue interruption,
and records a member as unavailable only after its exact command deadline
passes.

The real pinned-Gateway gate rotates independently signed certificates and
upstream targets, rejects the superseded certificate and selector, removes the
old certificate material, and recovers only the replacement after Gateway
restart. Same-policy validity renewal continues to retain the exact ACL digest
and active certificate until its successor is ready. Contract-level
mixed-version delivery, PostgreSQL replica-threshold recovery, and durable
Fleet command redispatch are verified. The coordinator that derives and
compiles a healthy target set for every member, real multi-Gateway failure
evidence, and joint production HA recovery remain open, so this foundation does
not complete `H0.2` or `H0`.

See the [Product Roadmap](ROADMAP.md) for dependencies, sub-gates, current
evidence, and the ordered product portfolio.

## Quick Start

### Requirements

- Rust 1.85 or later
- PostgreSQL 17 or a compatible supported release
- Docker for the first Runtime provider and real deployment gates
- The A3S Gateway source revision pinned in
  `tools/gateway-conformance/gateway-revision` for routed service operation
- Bun and Node.js 22 or later for the web console
- NATS JetStream only when the NATS event provider is selected

### Run the control plane

The development recipe creates an ephemeral bootstrap token when one is not
provided and keeps the API and web process under one signal boundary:

```bash
just cloud
```

Stop the pinned PostgreSQL, NATS, and registry dependencies separately:

```bash
just cloud-down
```

To run the API directly, start the development dependencies and provide the
required environment-backed credentials:

```bash
docker compose \
  --env-file deploy/dev/.env.example \
  --file deploy/dev/compose.yaml \
  up --detach --wait

export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:a3s_cloud@127.0.0.1:54320/a3s_cloud"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

Database migrations run during startup. The default development profile uses
the in-memory event provider. OpenAPI is available at
`http://127.0.0.1:8080/api/v1/openapi.json`.

### Bootstrap an organization

The caller creates and retains the first API token. Cloud stores only its
SHA-256 digest.

```bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: ${A3S_CLOUD_BOOTSTRAP_TOKEN}" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"${A3S_CLOUD_ADMIN_TOKEN}\",\"expiresAt\":null}"
```

Subsequent requests use
`Authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}`. Every mutation also requires a
stable `idempotency-key` header. Use OpenAPI and the web console for the current
resource and operation surfaces instead of treating README examples as a second
API specification.

## Platform Model

### Tenancy

```text
Organization
└── Project
    └── Environment
        ├── sources and BuildRuns
        ├── desired workload revisions
        ├── deployments and operations
        └── routes, Secrets, and observations
```

Authentication is global except for bootstrap and health routes. API tokens are
bound to an organization unless they carry the platform-administrator role.
Commands and queries enforce tenant ownership and scope at the application
boundary.

### Runtime boundary

A3S Runtime exposes two provider-neutral lifecycle classes:

| Class | Purpose |
| --- | --- |
| Task | Finite work such as a build, migration, evaluation, or backup |
| Service | Long-running work such as an application, Agent, MCP server, or model backend |

Runtime owns capability discovery and idempotent `apply`, `inspect`, `stop`, and
`remove` mechanics. Cloud owns resource identity, desired state, placement,
deployment workflows, release provenance, routing, and convergence decisions.

Applications use this path today. Agent, MCP, and Skill publication remains the
planned `A0` release profile; stateful resources remain `S0`; replicas and
multi-node placement remain `H0`; accelerator and inference capabilities remain
`I0`. These profiles do not create separate schedulers.

## Delivery Model

### Durable operations

```text
API command
  -> commit desired state + outbox fact in PostgreSQL
  -> create or locate an idempotent A3S Flow operation
  -> lease work to a reconciler or outbound node
  -> apply through Runtime or Gateway
  -> record exact observations and acknowledgements
  -> rebuild query and SSE projections
```

PostgreSQL is the desired-state authority. A3S Flow owns durable operation
progress. The transactional outbox and A3S Event accelerate coordination but
are never the only recovery path. Reconcilers compare desired and observed
generations until success is proven or a terminal failure is recorded.

### Workload deployment

An accepted workload template becomes an immutable revision. Mutable OCI tags
are resolved before scheduling, and only the digest is persisted as deployment
authority. A deployment selects an eligible node, applies one Runtime Service,
waits for durable health evidence, publishes the required Gateway state, and
activates only after the matching edge acknowledgement.

Update and rollback use the same path. A candidate cannot replace the active
revision until Runtime health and Gateway cutover succeed. Rollback clones a
previously activated template into a new monotonically increasing generation;
history is never rewritten.

### Source-to-workload delivery

The current `G0` path is:

```text
GitHub reference
  -> verified immutable commit and versioned recipe
  -> tenant-owned BuildRun and cloud.build@3 operation
  -> bounded exact checkout and content-addressed Artifact
  -> isolated Runtime Task and BuildKit build
  -> complete OCI and trusted retry-cache graph validation
  -> deterministic digest-only registry publication
  -> deterministic SPDX/SLSA evidence and locally verified DSSE signature
  -> explicit cloud.deployment@2 workload handoff
```

Private access uses short-lived GitHub App credentials that are revalidated for
the exact installation, account, and repository. Build cancellation and retry
remain durable operations; retry creates a new attempt while retaining the
source revision and lineage. Retry cache reuse cannot bypass OCI validation,
publication, evidence generation, signing, or local verification. Node-local
Artifact locations, signing private keys, and provider credentials are not part
of the public BuildRun state.

The detailed request contracts, failure boundaries, and acceptance evidence
remain in the [Development Plan](docs/development-plan.md).

## Gateway Relationship

| Product | Position | Owns |
| --- | --- | --- |
| A3S Cloud | Desired-state control plane | Tenancy, identity, catalogs, Workloads, replicas, placement, rollout, autoscaling, complete traffic policy, operations, usage ledger, and management surfaces |
| A3S Gateway | Traffic and protocol data plane | Transport, TLS, streaming, local policy enforcement, healthy endpoint selection, atomic configuration application, and request-path telemetry |

Cloud never proxies provider bytes or becomes a synchronous authorization
dependency. Gateway never becomes a tenant database, scheduler, production
rollout controller, production autoscaling authority, or long-term usage
ledger.

The Cloud-to-Gateway bridge compiles one complete ACL snapshot, binds it to the
target Gateway identity, revision, expected revision, exact ACL digest, issue
time, and expiry, and delivers it through an outbound node command. The node
agent calls Gateway's native snapshot apply endpoint and then queries exact
readiness. It emits an applied acknowledgement only when Gateway returns the
same snapshot metadata and `ready` state; rejection, expiry, mismatched status,
or unavailable readiness cannot advance Cloud state.

The node agent first selects `a3s.gateway.management-protocol.v1` from the
versioned Gateway descriptor. A legacy Gateway that exposes the exact
pre-descriptor v1 version response remains compatible, while an unknown
descriptor or mismatched request/status schema fails before snapshot apply.
The v4 Gateway acknowledgement and v2 command acknowledgement persist the
selected tuple and discovery mode. New control planes continue to read the old
v3/v1 acknowledgement pair, whose rows retain `NULL` protocol evidence rather
than a fabricated backfill.

Cloud owns a logical Gateway scope inside one organization, project, and
environment. The scope stores ordered desired physical membership, a
membership generation, and readiness policy. Its first member remains the
bootstrap primary for the current node-local route compiler. Each route carries
both logical and physical identities together with the exact workload revision,
deterministic Runtime unit identity, positive generation, port, canonical
node-local origin, and command-bound observation time. Those fields are durable
Cloud state and part of the compiled ACL digest; Gateway receives the resulting
complete policy but does not infer a target, interpret the logical scope, or
store Cloud tenancy.

Gateway's native journal is the sole source of truth for applied snapshot
state. The node agent does not maintain a second installed-snapshot CAS file,
so command redelivery and process restart converge through Gateway's idempotent
apply and status contract. This is the Gateway-native foundation of `H0.2`;
the periodic Edge reconciler renews an unchanged applied ACL before expiry,
reuses the existing certificate files, and keeps the prior revision
authoritative when renewal is rejected. Logical-scope ownership, migration,
same-environment/node enforcement, generation-bound target replacement,
PostgreSQL recovery, and real certificate/target rotation are verified for the
bootstrap-primary route path. Mixed-version delivery is verified at the
protocol boundary. Cloud also persists independent per-member rollout evidence
and computes ready, succeeded, or degraded aggregate outcomes without assuming
a global atomic reload. Its rollout reconciler recovers pending Fleet command
dispatch after restart and turns deadline expiry into explicit unavailable
evidence. Per-member healthy target compilation, real Gateway loss and
partial-availability evidence, and joint production HA recovery remain to be
implemented and verified.

Standalone Gateway remains independent with operator-owned ACL desired state.
In `cloud-managed` mode, Gateway rejects local providers and local scaling or
rollout blocks; Cloud is the sole production authority for those decisions.

## Architecture

A3S Cloud is a modular monolith with a separate outbound-only node agent. API,
worker, and event-relay roles can run in one control-plane process or as
independent roles from the same binary.

```text
browser / API client
        |
        v
A3S Boot control-plane API
        |
        +----> DDD application modules ----> PostgreSQL
        |              |                         |
        |              +----> A3S Flow <---------+
        |              +----> outbox -> A3S Event
        |
        v  outbound mTLS command lease
node agent
        +----> A3S Runtime ----> Docker / containerd / A3S Box
        +----> A3S Gateway ----> active edge revision
        +----> inventories, observations, and durable acknowledgements
```

| Component | Responsibility |
| --- | --- |
| A3S Boot | Modular API, dependency injection, CQRS, authentication, health, and OpenAPI |
| A3S ORM | Typed PostgreSQL access, transactions, and migrations |
| A3S Flow | Durable operations, retries, timers, and worker leases |
| A3S Event | Integration-fact delivery through local or NATS providers |
| A3S Runtime | Provider-neutral Task and Service lifecycle |
| A3S Gateway | HTTPS, routing, health, native snapshot application, and durable applied-state recovery |
| A3S ACL | Closed product configuration and validated manifests |

Business modules follow domain, application, infrastructure, and presentation
layers. Domain code remains independent of A3S Boot, SQL, HTTP, Runtime, Flow,
Event, and provider SDKs; infrastructure adapters implement ports owned by the
inner layers.

See [Technical Architecture](docs/architecture.md) for consistency ownership,
the node protocol, security boundaries, and recovery behavior.

## Configuration

Cloud and the node agent use closed, validated A3S ACL. Unknown fields and
unsafe timing relationships fail before the corresponding process starts.
Secrets are referenced through environment-variable names or Secret resources;
credential values do not belong in ACL.

| Configuration area | Responsibility |
| --- | --- |
| `server`, `auth`, `postgres` | API role, bootstrap, and durable state |
| `events`, `operations` | Outbox publication and durable operation timing |
| `node_control`, `fleet` | Outbound mTLS protocol, leases, inventories, and observations |
| `deployments`, `builds`, `artifacts` | Workload and source-build execution bounds |
| `registry`, `sources` | OCI publication and GitHub delivery policy |
| `edge`, `gateway` | Route compilation, certificates, snapshot validity, and node-local native Gateway application |
| `logs` | Durable log object storage, paging, retention, and compaction |
| `security` | Development or production PKI and encryption providers |
| `docker` | Node-local Docker and transient Secret materialization policy |

Use [control-plane configuration](config/cloud.acl) and
[node-agent configuration](config/node.example.acl) as the executable
references. The production security profile requires external Vault-backed
identity, Gateway certificate signing, and Secret encryption; production log
storage requires the configured S3-compatible adapter.

## Repository

Cloud is an application-local Rust workspace:

```text
Cloud/
├── ROADMAP.md
├── config/
│   ├── cloud.acl
│   └── node.example.acl
├── crates/
│   ├── contracts/
│   ├── control-plane/
│   ├── node-agent/
│   └── web-server/
├── deploy/
├── docs/
├── migrations/
├── tools/
└── web/
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

Run web checks from `web/`:

```bash
bun install --frozen-lockfile
bun run typecheck
bun run format:check
bun run lint:check
bun run test
bun run build
```

Real-provider and release certification must run on an isolated Linux host.
Use the repository-owned instructions rather than copying partial commands from
the README:

- [Runtime Conformance](tools/runtime-conformance/README.md)
- [Clean-Host Release Conformance](tools/release-conformance/README.md)
- [Production Web Delivery](deploy/web/README.md)

Design and delivery references:

- [Product Roadmap](ROADMAP.md)
- [Development Plan](docs/development-plan.md)
- [Domain Model](docs/domain-model.md)
- [Technical Architecture](docs/architecture.md)
- [Inference Plan](docs/inference-plan.md)

## License

MIT
