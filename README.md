# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud is an Agent-first service platform built on one governed runtime" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.77.0" src="https://img.shields.io/badge/REST_contract-1.77.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#service-model">Services</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#platform-capabilities">Capabilities</a> &middot;
  <a href="#delivery-status">Delivery</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#documentation">Docs</a>
</p>

**A3S Cloud is a self-hosted, Agent-first developer platform for AaaS, WaaS,
FaaS, Durable Cell collaboration, model inference, and tenant Web delivery on
operator-owned CPU/GPU infrastructure.** Product intent is governed by Cloud,
durably coordinated by A3S Flow, projected to the provider-neutral A3S Runtime,
executed through A3S Box, and exposed only through A3S Gateway.

> [!IMPORTANT]
> Architecture targets are not availability claims. A capability is released
> only after its real-provider, failure, recovery, cleanup, upgrade, and release
> gates are `Verified` in [ROADMAP.md](ROADMAP.md).

> [!NOTE]
> A3S Cloud does not ship its own management Dashboard. It does host immutable
> React/Vue and other static releases for tenant Applications and Agents; those
> sites use the same public Gateway and APIs as every other client.

## Service model

Cloud presents six first-class service outcomes without creating six execution
stacks:

| Service | Semantic owner | Execution projection |
| --- | --- | --- |
| **AaaS — Agent as a Service** | Agents owns conversations, executions, semantic events, approvals, checkpoints, forks, provider bindings, and recovery | Warm stateful Runtime `Service`; bounded batch Agents may use `Task` |
| **WaaS — Workflow as a Service** | Workflow owns ontology, immutable definitions and plans, WorkflowRun, HumanTask, typed node order, and outcomes | No Workflow Runtime class; A3S Flow coordinates nodes that call Agents, Functions, MCP, Inference, Cells, Connectors, Tasks, or Services |
| **FaaS — Function as a Service** | Functions owns immutable release/profile and invocation semantics | Runtime `Task`, stateless Runtime `Service`, or an external FaaS Connector; sessionless MCP can use the Service or external mode |
| **Durable Cell** | Durable Cells owns application revision, compatibility, retention, and deployment/storage correlation | An ordinary Runtime `Service`; provider-owned named state supplies serialized human/multi-Agent collaboration, alarms, presence, and hibernatable sessions |
| **Model Inference** | Inference owns model revision, deployment, role topology, routing policy, usage, and evaluation | Runtime `Service` replicas or typed multi-node prefill/decode groups scheduled on the shared GPU/CPU rail |
| **Static Web** | Applications/Assets owns the immutable Web release and binding | Build in a Runtime `Task`; serve admitted objects directly through Gateway with SPA fallback—no per-site Service unless SSR is explicitly selected |

The only general execution classes are **Task** and **Service**. Agent,
Function, MCP, inference, and Cell behavior is expressed through immutable
consumer-owned profiles, not new Runtime classes. A3S Runtime owns the unified
lifecycle contract and is implemented by A3S Box providers.

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud target architecture from Gateway through product domains, durable control, scheduling, Runtime and Box, supply, data, and observability" />
</p>

The system follows five non-negotiable invariants:

1. Accepted intent is durable before asynchronous work starts.
2. Every decision and mutable datum has exactly one owning bounded context.
3. Dependencies point inward; side effects stay behind consumer-owned ports.
4. Desired state advances only from exact identity-, generation-, and
   profile-bound evidence.
5. Every side effect has one retry, recovery, cancellation, cleanup, and
   fencing owner.

### Four paths, one authority map

- **North-south traffic:** A3S Gateway is the only public entry for Cloud APIs,
  Agent/Workflow/Function/MCP/model endpoints, tenant sites, TLS, traffic
  policy, and rate enforcement. Cloud processes remain private.
- **Control and recovery:** authorized commands commit desired state,
  idempotency, an Operation, audit evidence, and Outbox facts in PostgreSQL.
  A3S Flow owns durable waits, retry, replay, compensation, and cancellation.
- **Placement and execution:** Workloads and Fleet own the single CPU/GPU
  scheduler, Claims, rollout, node commands, receipts, drain, and fencing. A
  node agent converges Runtime `Task`/`Service` units through A3S Box.
- **Live serving:** Edge compiles complete versioned snapshots; Gateway applies
  and serves them. Cloud never becomes a second request-byte proxy.

Runtime CI/CD forms a source-to-release control path across these authorities:
build once, verify the exact digest, promote the same release, deploy through
Workloads/Fleet, shift traffic through Edge/Gateway, and roll back to a prior
admitted release. A3S Flow owns pipeline history; product release owners retain
release truth.

## Platform capabilities

### Agent and application platform

- Stateful Agents with provider-neutral Harness bindings, approvals,
  checkpoint/fork trajectories, exact Tool evidence, and recovery.
- Ontology- and graph-defined Workflows that compose Agent Runtime nodes,
  Functions, MCP, inference, Durable Cells, people, and external Connectors.
- Hosted and external FaaS, including sessionless MCP profiles and Function
  calls from A3S Code.
- First-class Durable Cell spaces for people and multiple Agents sharing
  serialized, low-latency state without copying Agent or Workflow history.
- Immutable React/Vue/static UI releases bound to Applications or Agents and
  delivered by Gateway; SSR is an ordinary Service profile.

### Compute and runtime delivery

- One heterogeneous scheduler for CPU pools, GPU pools, topology, resource
  Claims, anti-affinity, gang placement, maintenance, and preemption policy.
- Stateless scale-out and scale-to-zero; stateful drain, single-writer fences,
  checkpoint/handoff, recovery, and locality-aware placement.
- Distributed inference with independent replicas, multi-node groups, and
  prefill/decode-disaggregated roles; KV-transfer and accelerator topology are
  explicit constraints rather than a second scheduler.
- One Flow-backed CI/CD model for Agent, Workflow, Function/MCP, Cell,
  inference, Static Web, and Cloud system-service releases.

### Content and supply

- Hosted Git source authority, external source revisions and webhooks,
  reproducible Box builds, provenance, previews, and immutable artifacts.
- Separate **OCI Registry**, **A3S Use Registry**, and **Git** authorities;
  none is overloaded to impersonate another content type.
- Governed logical Models and Model Revisions plus immutable model-weight
  manifests/objects, external hub resolution such as ModelScope, licenses,
  trust policy, and reconstructible node caches.
- One typed immutable-object client over external HTTPS AWS S3 or
  S3-compatible storage. Cloud does not bundle an S3 server or pretend object
  storage is a POSIX/FUSE filesystem. Mutable volumes, backup, restore,
  retention, and writer fencing belong to Data/S0.

### Developer platform and governance

- One persisted immutable Installation identity and one exact discriminated
  Installation/Organization/Project/Environment scope contract across tenant
  isolation, memberships, grants, quotas, credentials, audit, Outbox, and
  lifecycle cleanup. Platform facts never borrow a sentinel Organization.
- A distinct system-administrator RBAC plane for installation, fleet,
  migration, policy, incident, and break-glass duties; system roles do not
  silently become tenant access. Tenant support requires an active exact human,
  an admitted support-use role, a short-lived non-renewing grant, descendant
  scope and one closed non-sensitive permission; the allow fact pins replayable
  policy/grant evidence.
- REST/OpenAPI, maintained TypeScript client, CLI, and sessionless Management
  MCP all dispatch the same Application commands and queries.
- OpenShift-class outcomes—declarative reconciliation, scheduling, isolation,
  rollout, policy, observability, and day-two operations—and TokenHub-class
  outcomes—governed model/provider/key access, routing, quotas, diagnostics,
  and usage—implemented through A3S authorities rather than copied control
  planes or APIs.

### Distributed consistency and operations

| Concern | Rule |
| --- | --- |
| Command concurrency | Tenant scope, idempotency key, expected aggregate version, and payload digest are checked transactionally; conflicting replay fails closed |
| Database writes | Aggregate, idempotency, audit, and Outbox commit together through A3S ORM/PostgreSQL; the database resolves each fact to its canonical Installation lineage, and no distributed transaction spans an external provider |
| Cross-system work | A3S Flow sagas and owner receipts reconcile uncertain outcomes; transactional Outbox publishes only committed facts |
| Rate limits | Gateway enforces request limits; owner admission enforces durable quota. Redis may accelerate counters but is never quota truth |
| Cache | Redis contains bounded, reconstructible reads, discovery, tokens, and coordination hints with revisioned invalidation; cache loss changes latency, not correctness |
| Locks and leases | PostgreSQL/CAS owns correctness and fencing. Distributed locks reduce contention only and cannot replace aggregate versions or Claims |
| Dispatch pressure | A3S Lane admits only post-durable work for fairness, backpressure, and bounded concurrency; it does not own workflow or queue truth |
| Telemetry | OpenTelemetry carries correlated logs, metrics, and traces; immutable evidence remains with owners, while Apache Doris is an optional rebuildable analytics/SLO projection |

## DDD and mechanism ownership

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="DDD dependency direction and owner port rules in A3S Cloud" />
</p>

Presentation calls Application; Application coordinates its Domain and
consumer-owned ports; Infrastructure implements those inward ports. A context
may cross another boundary only through a synchronous owner Application
contract or a versioned fact emitted from the owner's committed Outbox.

| Concern | Sole authority | Forbidden duplicate |
| --- | --- | --- |
| Tenant identity and authorization | Identity + Projects | Adapter-local roles, UI-only policy, or provider sessions as truth |
| Product meaning | Owning Agent, Workflow, Function, Cell, Inference, Application, or Asset context | Runtime/provider fields becoming product state |
| Durable coordination | Operations + A3S Flow | Product retry tables, sleep loops, or another workflow engine |
| Build and release delivery | Sources + Artifacts + product Release owner + Delivery Pipelines | Product-local CI state, rebuild-on-promotion, or mutable deployment tags |
| Placement and rollout | Workloads + Fleet | Agent-, MCP-, Cell-, model-, or Gateway-specific scheduler |
| Provider lifecycle | A3S Runtime + A3S Box | Direct process/container/FaaS calls from product domains |
| Public traffic | Edge desired state + A3S Gateway applied state | Cloud proxy, per-product ingress, or a second Gateway publisher |
| Immutable and mutable data | Shared object client + Data/S0 | Per-product S3 clients, backup engines, or provider state as desired-state truth |
| Integration facts | One scope-aware transactional Outbox + A3S Event | Publish-before-commit, sentinel tenants, or product/platform-local event buses |
| Configuration | A3S ACL parsed by `a3s-acl` | Compatibility parsers or another product configuration language |

Cross-cutting behavior is an explicit ordered pipeline—authentication,
authorization, validation, idempotency, transaction, audit/Outbox, then
dispatch—not hidden domain mutation. Logging, tracing, metrics, cache, and
rate-limit adapters observe or protect that pipeline but cannot become a
second business authority. The executable architecture ratchets in
[the architecture audit](docs/architecture-audit.md) prevent outer-layer
imports and duplicate mappings from spreading while existing debt is removed.

## Delivery status

The portfolio is gate-driven, not percentage-driven. As of **2026-08-29**:

| Lane | Status |
| --- | --- |
| Tenant-scoped PostgreSQL identity, ORM-backed Operations/Flow, Outbox, API, and migration authority | **Verified foundation** |
| Installation scope and system-administrator RBAC | **Foundation in progress**; one persisted immutable Installation identity, explicit scope hierarchy, shared scope-aware Audit/Outbox rail, canonical platform-role policy/bindings, bounded tenant-support grants, and replayable privileged-decision evidence now have sole PostgreSQL repositories, current-head/approval invariants, and hostile multi-replica gates. Every non-bootstrap RBAC or support mutation consumes the exact verified credential ID, issues its closed authorization decision in the same transaction as the protected write, derives authentication evidence server-side, and links the business Audit fact to that decision. Production completion still requires maintained Application/REST/OpenAPI/client/CLI/MCP interfaces and replacement of every legacy cross-surface administrator bypass; no generic caller-authored evaluator is exposed |
| Node, Workload, Runtime/Box, Gateway, supply, collaboration, and enterprise controls | **In progress**; several component gates exist, current real-provider/release recertification remains |
| Agent and hosted MCP product lanes | **In progress**; do not infer complete AaaS availability from component evidence |
| Ontology Workflow and AI Applications/Files foundations | **In progress**; complete WaaS/Application products remain gate-bound |
| Data/S0 and Durable Cell | **Foundation in progress**; Durable Cell service is not yet available |
| FaaS, distributed inference, model supply, Static Web, Runtime CI/CD, workload identity, and full HA operations | **Planned or early foundation**; workload identity has a component-only canonical WI1 contract and remains unavailable pending persistence, attestation, issuance, enforcement, and provider evidence |

See the [product roadmap](ROADMAP.md), [platform gap analysis](docs/platform-gap-analysis.md),
and [ecosystem project roadmaps](docs/project-roadmaps/README.md) for exact
dependencies, evidence, and remaining work.

## Deployment model

Cloud system services and tenant workloads share mechanisms but not authority:

- the bootstrap plane installs PostgreSQL, NATS, S3-compatible storage, Git,
  OCI Registry, A3S Use Registry, migrator, API, Worker, Relay, Gateway, and
  observability dependencies through one dependency DAG;
- API, Worker, Relay, migrator, node agent, and Gateway are independently
  scalable roles with explicit readiness, leader/lease, migration, and rollout
  contracts;
- tenant workloads enter only through admitted releases, Workloads/Fleet
  placement, Runtime/Box execution, and Gateway publication;
- management is API/OpenAPI/client/CLI/MCP-first. No Cloud Dashboard or
  UI-specific backend is part of the platform.

The initial Box-hosted profile is the installation foundation. Production HA
requires the named clean-install, upgrade, rollback, dependency-loss,
credential-rotation, storage recovery, node-drain, and multi-replica gates in
the [deployment architecture](docs/deployment-and-cluster-architecture.md).

## Quick start

### Requirements

- Rust 1.88 or later
- PostgreSQL 17 or a compatible supported release
- Git CLI for pinned external-source acquisition
- A3S Box for node-local workload/build execution
- the pinned A3S Gateway revision for routed services
- NATS JetStream for production `all`, Worker, or Relay roles
- Bun only for TypeScript client or CLI development

### Start the development API

```bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_POSTGRES_MIGRATION_URL="$A3S_CLOUD_POSTGRES_URL"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

Serving processes never run migrations. The one-shot migrator runs after
PostgreSQL is reachable and before API, Worker, Relay, or `all`; production
uses distinct migration and serving principals.

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
```

Direct port access is a local-development convenience. A production deployment
publishes the API only through A3S Gateway.

<details>
<summary><strong>Bootstrap the first organization</strong></summary>

Cloud stores only the API-token digest; the caller creates and retains the
credential.

```bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: ${A3S_CLOUD_BOOTSTRAP_TOKEN}" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"${A3S_CLOUD_ADMIN_TOKEN}\",\"expiresAt\":null}"
```

Subsequent mutations use `Authorization: Bearer ...` and a stable
`idempotency-key`.

</details>

### Use the CLI

```bash
bun install --cwd cli --frozen-lockfile

export A3S_CLOUD_TOKEN="${A3S_CLOUD_ADMIN_TOKEN}"
export A3S_CLOUD_URL="http://127.0.0.1:8080/api/v1"

bun run --cwd cli src/main.ts diagnostics status --output=json
bun run --cwd cli src/main.ts organizations list --output=json
bun run --cwd cli src/main.ts operations list --output=json
```

Credentials come from environment variables or standard input and are never
written to a CLI context file.

## Interfaces

| Surface | Contract | Start here |
| --- | --- | --- |
| REST/OpenAPI | Versioned `/api/v1`, request IDs, idempotency, common envelopes, committed snapshot | [Guide](docs/openapi.md) · [`openapi/v1.json`](openapi/v1.json) |
| TypeScript client | Maintained adapter over the same REST contract | [`packages/cloud-client`](packages/cloud-client) |
| CLI | Scriptable structured output with no token argument | [`cli/README.md`](cli/README.md) |
| Management MCP | Sessionless, tenant-authorized tools over the same commands and queries | [`docs/management-mcp.md`](docs/management-mcp.md) |

## Configuration

Cloud and the Node Agent accept only closed, validated **A3S ACL**. Unknown
fields and unsafe timing relationships fail before startup; Secret values never
belong in ACL. Start with [`config/cloud.acl`](config/cloud.acl),
[`config/node.example.acl`](config/node.example.acl), and the
[`deploy/production`](deploy/production/README.md) baseline.

Redis is optional acceleration, not durable truth. Doris is optional analytics,
not an operational database. S3-compatible object storage and NATS are external
deployment dependencies in production profiles.

## Repository

```text
Cloud/
|-- crates/
|   |-- contracts/       # versioned cross-process contracts
|   |-- control-plane/   # bounded contexts, API, workers, persistence
|   `-- node-agent/      # outbound node protocol and provider adapters
|-- migrations/          # PostgreSQL schema evolution
|-- config/              # closed A3S ACL configuration
|-- openapi/             # committed REST contract
|-- packages/cloud-client/
|-- cli/
|-- tools/               # provider, recovery, architecture, release gates
`-- docs/                # architecture, decisions, plans, and runbooks
```

## Development

```bash
cargo fmt --all -- --check
cargo test -p a3s-cloud-control-plane architecture_tests --lib
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Real-provider and release certification runs on isolated Linux hosts. Important
repository-owned gates include [cross-surface conformance](tools/c0-conformance/README.md),
[Runtime conformance](tools/runtime-conformance/README.md),
[Box provider conformance](tools/box-conformance/README.md), and the
[pinned Gateway revision](tools/gateway-conformance/gateway-revision).

## Documentation

| Start here | Purpose |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, and delivery order |
| [Technical architecture](docs/architecture.md) | Stable ownership, topology, consistency, and failure behavior |
| [AI service platform](docs/ai-service-platform-architecture.md) | AaaS, WaaS, FaaS, Durable Cell, Inference, Runtime, Box, and Gateway composition |
| [Agent Runtime](docs/agent-runtime-architecture.md) · [Function Runtime](docs/function-runtime-architecture.md) · [Durable Cell](docs/durable-cell-platform-plan.md) | Service-specific semantics over the unified Runtime |
| [Static Web](docs/static-web-hosting-architecture.md) · [model supply](docs/model-supply-architecture.md) · [inference](docs/inference-plan.md) | Tenant UI, models/weights, and serving architecture |
| [Cluster deployment](docs/deployment-and-cluster-architecture.md) · [elastic services](docs/elastic-service-deployment-architecture.md) | System services, CPU/GPU scheduling, stateful/stateless convergence, HA |
| [Runtime CI/CD](docs/runtime-cicd-architecture.md) · [workload identity](docs/workload-identity-and-service-connectivity-architecture.md) | Delivery, attestation, private discovery, mTLS, and revocation |
| [Distributed API consistency](docs/distributed-api-consistency-architecture.md) · [Redis and Lane](docs/redis-and-lane-platform-architecture.md) | Concurrency, transactions, cache, locks, fairness, and backpressure |
| [Observability and analytics](docs/observability-and-analytics-architecture.md) · [platform gap analysis](docs/platform-gap-analysis.md) | Telemetry/SLO/incident design and prioritized missing outcomes |
| [Multi-tenant platform](docs/multi-tenant-developer-platform-architecture.md) · [capability architecture](docs/platform-capability-architecture.md) | Tenant/admin RBAC and OpenShift-/TokenHub-class outcomes |
| [DDD, AOP, and patterns](docs/ddd-aop-and-pattern-architecture.md) · [architecture audit](docs/architecture-audit.md) | Layer rules, aspect order, patterns, and executable debt ratchets |
| [Ecosystem roadmaps](docs/project-roadmaps/README.md) | Mission, dependencies, evidence, and negative boundary for every A3S subproject |

## License

[MIT](LICENSE)
