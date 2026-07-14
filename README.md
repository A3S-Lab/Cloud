# A3S Cloud

<p align="center">
  <strong>Self-Hosted Application and Agent Platform</strong>
</p>

<p align="center">
  <em>Deploy applications and A3S assets to operator-owned Linux nodes with durable operations, observable convergence, and safe rollback</em>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#product-loop">Product Loop</a> •
  <a href="#planned-capabilities">Capabilities</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#asset-model">Asset Model</a> •
  <a href="#mvp-roadmap">MVP Roadmap</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Cloud** is a planned self-hosted control plane for running applications,
Agents, MCP servers, and Skill-enabled workloads on Linux infrastructure owned
by the operator. It combines a project-oriented deployment experience with A3S
Runtime, Flow, Event, Gateway, Boot, ORM, and ACL contracts.

Cloud is designed around convergence rather than request-time orchestration. A
command commits desired state, a durable operation advances the deployment, a
node reports observed Runtime state, and the control plane reconciles the two
until the requested outcome is proven or fails explicitly.

> **Project status:** this repository currently contains architecture and
> delivery design. The control plane, node agent, web application, database
> schema, and Runtime provider integrations are not implemented yet.

## Product Loop

The first release proves one complete path before expanding the platform:

```text
enroll a Linux node
  -> create a project and environment
  -> deploy a digest-pinned OCI image
  -> verify health through the real service path
  -> publish an HTTPS route
  -> stream ordered logs
  -> update or roll back to the previous healthy revision
```

Every step is idempotent and recoverable. A process restart, repeated command,
expired lease, or lost acknowledgement must converge on the same deployment
instead of creating a second provider resource.

## Planned Capabilities

- **Projects and Environments**: Organize desired state by tenant, project, and
  isolated environment.
- **Outbound Node Control**: Enroll Linux nodes with short-lived credentials and
  operate them through outbound mutually authenticated HTTPS.
- **Immutable Deployments**: Resolve sources once and deploy OCI manifest
  digests rather than mutable tags.
- **General A3S Runtime**: Run finite Tasks and long-running Services through one
  provider-neutral lifecycle contract.
- **Durable Operations**: Use A3S Flow for replayable deployment, rollback,
  build, backup, and repair operations.
- **Observed Convergence**: Keep desired state in PostgreSQL and accept success
  only after Runtime, health, and edge observations match it.
- **HTTPS Routing**: Publish complete, versioned A3S Gateway snapshots and wait
  for acknowledgement before exposing a route.
- **Logs and Activity**: Stream ordered logs with resumable cursors and show one
  correlated operation timeline in the web application.
- **Secrets**: Store immutable encrypted secret versions and deliver only the
  references required by a workload generation.
- **A3S Asset Releases**: Publish immutable Agent, MCP, and Skill releases with
  source, manifest, and artifact provenance.

### Capability ownership

| Capability | Authoritative component |
| --- | --- |
| Tenant, project, environment, and desired workload | PostgreSQL domain tables |
| Long-running operation history | A3S Flow PostgreSQL event store |
| Provider resource and live health | Node agent and A3S Runtime provider |
| Active edge configuration | A3S Gateway configuration revision |
| Integration fact delivery | Transactional outbox and A3S Event |
| OCI images and bundles | OCI registry or S3-compatible object storage |
| Hosted source repositories | Durable POSIX Git storage |

## Architecture

A3S Cloud begins as a modular monolith with a separate, outbound-only node
agent. The control-plane roles may run in one process for the first release and
separate later without changing the consistency model.

```text
                         A3S Cloud

 Web browser
      |
      v
 A3S Boot API ----> DDD application modules ----> PostgreSQL
      |                       |                         |
      |                       +----> A3S Flow <---------+
      |                       +----> transactional outbox
      |                                      |
      |                                      v
      |                                  A3S Event
      |
      | outbound mTLS command lease and observations
      v
 Node agent ----> A3S Runtime ----> Docker / containerd / A3S Box
      |
      +---------> A3S Gateway ----> healthy Service unit
```

### Control plane

The Rust control plane uses A3S Boot modules, typed dependency injection, CQRS,
request validation, OpenAPI, and lifecycle hooks. Business modules follow four
DDD layers:

```text
modules/{context}/
├── domain/                  # pure Rust entities, values, ports, events
├── application/             # commands and queries
├── infrastructure/          # PostgreSQL and external-provider adapters
├── presentation/            # thin controllers and transport DTOs
└── module.rs
```

Domain modules never import A3S Boot, SQL, HTTP, Runtime, Flow, Event, or a
provider SDK. Infrastructure implements typed ports owned by the application or
domain layer.

### Runtime boundary

A3S Runtime is a general execution boundary. Its core model is a `RuntimeUnit`
with one of two lifecycle classes:

| Class | Purpose |
| --- | --- |
| Task | Finite work such as a build, migration, evaluation, or backup |
| Service | Long-running work such as an application, Agent, or MCP server |

The Runtime core owns capability discovery and idempotent
`apply / inspect / stop / remove` operations. Product profiles, provider
selection policy, deployment workflows, routing, scheduling, and asset release
semantics stay in their owning layers.

### Consistency model

- PostgreSQL stores business desired state and transactional outbox rows.
- A3S Flow stores durable operation history and step leases.
- Node agents persist command outcomes and report Runtime observations.
- A3S Event distributes committed facts but is never the sole recovery path.
- Reconcilers periodically compare desired and observed generations.
- Gateway configuration is published as an atomic, versioned snapshot.
- Each external step owns an independent timeout and cancellation policy.

See [Technical Architecture](docs/architecture.md) for protocols, security,
middleware adoption, data ownership, and failure recovery.

## Asset Model

A3S Cloud publishes three first-class asset families:

| Asset | Release artifact | Runtime use |
| --- | --- | --- |
| Agent | Digest-pinned OCI artifact and validated manifest | Long-running Service or finite Task profile |
| MCP | Digest-pinned OCI artifact and validated manifest | Long-running MCP Service |
| Skill | Content-addressed bundle and validated manifest | Immutable input bound to an Agent Service |

A published release is immutable and binds its source commit, manifest digest,
and artifact digest. Release publication, catalog visibility, and deployment are
separate state transitions. Agent and MCP releases use the common Workload and
Deployment path; a Skill version is attached as an immutable workload input.

## Technology

### A3S components

| Component | Responsibility |
| --- | --- |
| A3S Boot | Modular HTTP control plane, DI, CQRS, validation, and OpenAPI |
| A3S ORM | Typed PostgreSQL queries, transactions, and migrations |
| A3S Runtime | Provider-neutral Task and Service lifecycle |
| A3S Flow | Durable operations, retries, timers, and worker leases |
| A3S Event | Integration-fact delivery through local or NATS providers |
| A3S Gateway | HTTPS, ACME, routing, health, and atomic reload |
| A3S ACL | Typed product configuration and asset manifests |

### Infrastructure providers

The first-node profile requires PostgreSQL, Docker, A3S Gateway, and access to
an OCI registry. Distributed production profiles add infrastructure only when
the corresponding capability needs it:

| Need | Provider direction |
| --- | --- |
| Distributed event consumers | NATS JetStream |
| Source builds | Rootless BuildKit and an owned OCI registry |
| Logs, release archives, and backups | S3-compatible object storage |
| Production key encryption and node PKI | OpenBao/Vault, cloud KMS, or step-ca |
| Metrics and traces | OpenTelemetry Collector and Prometheus-compatible storage |
| Stateful multi-node failover | A fenced attach/detach volume provider |

## MVP Roadmap

| Gate | Outcome |
| --- | --- |
| R0 — Universal Runtime | Task and Service contracts, capability matching, durable identity, and real Docker conformance |
| F0 — Foundation | Boot control plane, PostgreSQL, Flow, identity, projects, environments, outbox, and operation projections |
| N0 — Node control | Enrollment, mTLS, command leases, observations, command journal, and Docker driver |
| D0 — OCI deployment | Immutable workload revisions, one-node scheduling, apply, health, activation, stop, and cancellation |
| E0 — Reachable service | HTTPS route, logs, update, rollback, web timeline, and crash-recovery acceptance |
| A0 — Asset release | Minimal Agent/MCP release import, Skill bundle publication, and deployment through the common path |

The MVP remains single-control-plane, single-node, Docker-backed, and stateless.
Its completion gate is a repeatable end-to-end run on a clean Linux host, plus
crash injection at every durable boundary. See the
[Development Plan](docs/development-plan.md) for the full sequence and exit
criteria.

## Repository

Implementation will use an app-local Rust workspace. The A3S monorepo remains
an orchestration root rather than a Rust workspace.

```text
Cloud/
├── Cargo.toml                 # app-local workspace
├── config/
│   ├── cloud.hcl
│   └── node.example.hcl
├── migrations/
├── crates/
│   ├── contracts/            # versioned node and public protocol types
│   ├── control-plane/        # modular monolith: API, worker, reconciler
│   └── node-agent/           # outbound mTLS agent and Runtime adapters
├── web/                      # React control plane
├── deploy/
├── tests/
└── docs/
```

## Development

The implementation workspace has not been scaffolded yet, so there are no
build or test commands to advertise. Development begins with the universal
Runtime prerequisite and contract tests, followed by the control-plane and node
vertical slices described in the roadmap.

Design references:

- [Domain Model](docs/domain-model.md)
- [Technical Architecture](docs/architecture.md)
- [Development Plan](docs/development-plan.md)

All implementation changes must use focused tests, real provider integration
gates, formatting, Clippy, documentation checks, and clean-host end-to-end
validation before a capability is described as complete.

## License

Licensed under the [MIT License](LICENSE).
