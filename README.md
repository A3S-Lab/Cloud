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
  <a href="#mvp-roadmap">MVP Roadmap</a> •
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
- **Runtime Observations**: Record provider capabilities, workload state,
  health, logs, and durable command acknowledgements from A3S Runtime
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

### MVP capability matrix

| Area | Capability | State |
| --- | --- | --- |
| Runtime prerequisite | General Task and Service lifecycle with provider capability matching | Complete |
| Foundation | Identity, tenancy, PostgreSQL, Flow, outbox, projections, API, and web shell | Complete |
| Node control | Enrollment, node identity, outbound mTLS, command leases, and observations | Complete |
| Deployment | Digest-pinned OCI revisions, scheduling, apply, health, activation, stop, cancellation, and recovery | Complete |
| Reachability | Gateway snapshot publication is complete; HTTPS routes, ordered logs, update, rollback, and crash recovery remain | In progress |
| Releases | Immutable Agent, MCP, and Skill publication through the common deployment path | Planned |

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

cargo run -p a3s-cloud-control-plane -- config/cloud.hcl
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

Cloud validates a closed HCL configuration at startup. Unknown fields and
unsafe timing relationships fail before the API or worker starts. The shipped
D0 deployment policy is split across independent boundaries:

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
| `gateway.connect_timeout_ms` | Connection timeout for the node-local Gateway management API |
| `gateway.validation_timeout_ms` | Independent deadline for validating one complete snapshot |
| `gateway.reload_timeout_ms` | Independent deadline for transactionally reloading one snapshot |

These timers do not consume one shared request budget. API acceptance commits
desired state first; Flow, node command, Runtime, health, and cleanup deadlines
then advance independently. A mutable image tag is resolved before scheduling
and the resulting workload revision remains digest-addressable on replay.

See [`config/cloud.hcl`](config/cloud.hcl) and
[`config/node.example.hcl`](config/node.example.hcl) for the complete control
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
| A3S ACL | Closed HCL product configuration and validated manifests |

Business modules follow four DDD layers. Domain code remains independent of
A3S Boot, SQL, HTTP, Runtime, Flow, Event, and provider SDKs; infrastructure
adapters implement typed ports owned by the inner layers.

See [Technical Architecture](docs/architecture.md) for the node protocol,
security model, consistency boundaries, and failure recovery.

## MVP Roadmap

| Gate | Outcome | State |
| --- | --- | --- |
| R0 — Universal Runtime | General Task and Service contracts, durable identity, capability matching, and Docker conformance | Verified |
| F0 — Foundation | Boot control plane, PostgreSQL, identity, tenancy, Flow operations, outbox, projections, and web shell | Verified |
| N0 — Node control | Enrollment, mTLS, command leases, observations, command journal, and Docker driver | Verified |
| D0 — OCI deployment | Immutable workload revisions, one-node scheduling, apply, health, activation, stop, cancellation, and recovery | Verified |
| E0 — Reachable service | Gateway snapshot transport is verified; HTTPS route, ordered logs, update, rollback, web timeline, and crash-recovery acceptance remain | In progress |
| A0 — Release catalog | Agent and MCP release import, Skill bundle publication, and deployment through the common path | Planned |

The MVP target is a single control plane, one Linux node, Docker-backed
stateless workloads, and a repeatable end-to-end deployment on a clean host. The
exit gate includes crash injection at each durable boundary, recovery without
duplicate provider resources, and rollback to the previous healthy revision.

See the [Development Plan](docs/development-plan.md) for milestone order and
acceptance criteria.

## Repository

Cloud is an app-local Rust workspace inside the A3S monorepo:

```text
Cloud/
├── Cargo.toml
├── config/
│   ├── cloud.hcl
│   └── node.example.hcl
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
cargo test -p a3s-cloud-node-agent --test docker_conformance -- --nocapture

A3S_CLOUD_TEST_DOCKER=1 \
cargo test -p a3s-cloud-control-plane --test docker_deployment -- --nocapture

A3S_CLOUD_TEST_REGISTRY_URL="http://127.0.0.1:50020/" \
cargo test -p a3s-cloud-control-plane --test oci_registry_integration -- --nocapture

A3S_CLOUD_TEST_GATEWAY_BIN="$(command -v a3s-gateway)" \
cargo test -p a3s-cloud-node-agent \
  installed_a3s_gateway_validates_and_reloads_complete_snapshots -- --nocapture
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

Design references:

- [Domain Model](docs/domain-model.md)
- [Technical Architecture](docs/architecture.md)
- [Development Plan](docs/development-plan.md)

## License

MIT
