# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud turns Agent, Workflow, Function, Durable Cell, inference, and Web semantics into governed services on A3S Runtime and Box" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.81.0" src="https://img.shields.io/badge/REST_contract-1.81.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#how-it-works">Architecture</a> &middot;
  <a href="#service-outcomes">Services</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#platform-capabilities">Capabilities</a> &middot;
  <a href="#delivery-status">Delivery</a> &middot;
  <a href="#documentation">Docs</a>
</p>

**A3S Cloud is a self-hosted, Agent-first developer platform that turns
tenant-authorized product intent into durable AaaS, WaaS, FaaS, Durable Cell,
model-inference, and Static Web services on operator-owned CPU/GPU
infrastructure.** Cloud owns product and desired-state truth; A3S Flow
coordinates durable work; A3S Runtime defines one lifecycle contract; A3S Box
executes it; A3S Gateway is the only public edge.

> [!IMPORTANT]
> Architecture targets are not availability claims. A capability is released
> only after its real-provider, failure, recovery, cleanup, upgrade, and release
> gates are marked <code>Verified</code> in [ROADMAP.md](ROADMAP.md).

> [!NOTE]
> A3S Cloud does not ship a management Dashboard. It does host immutable
> React/Vue and other tenant Web releases for Applications and Agents. Those
> sites use the same Gateway and APIs as every other client.

## How it works

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud authority map from A3S Gateway through tenant product domains, Identity, PostgreSQL, Flow, Workloads and Fleet, Runtime, Box, supply, storage, dispatch, and observability" />
</p>

The architecture follows one path for every service:

1. **Admit:** A3S Gateway authenticates public traffic; Identity resolves the
   exact Installation, Organization, Project, Environment, Principal, and
   credential scope.
2. **Commit:** an Application use case atomically writes desired state,
   idempotency, audit evidence, and Outbox facts to PostgreSQL through A3S ORM.
3. **Coordinate:** A3S Flow and Operations own durable waits, retry, replay,
   approval, compensation, cancellation, and delivery history.
4. **Place and execute:** Workloads and Fleet own the single CPU/GPU scheduler,
   Claims, rollout, drain, and fencing; the node agent converges A3S Runtime
   <code>Task</code> or <code>Service</code> units through A3S Box.
5. **Publish:** Edge compiles complete versioned route snapshots; A3S Gateway
   applies and serves them. Cloud never becomes a second request-byte proxy.

Runtime CI/CD uses the same authority map: build once, verify the exact digest,
promote the same immutable release, deploy through Workloads/Fleet, shift
traffic through Edge/Gateway, and roll back to an earlier admitted release.
Product domains retain release truth; Flow retains pipeline history.

## Service outcomes

Six product outcomes share two execution classes instead of creating six
runtime stacks:

- **AaaS — Agent as a Service.** Agents owns conversations, executions,
  semantic events, approvals, checkpoints, forks, Tool evidence, provider
  bindings, and recovery. Stateful Agents such as A3S Code run as warm,
  session-fenced Runtime <code>Service</code> units; bounded batch Agents may
  use <code>Task</code>.
- **WaaS — Workflow as a Service.** Workflow owns ontology, immutable
  definitions and plans, WorkflowRun, HumanTask, typed node order, and
  outcomes. A3S Flow coordinates Agent, Function, MCP, Inference, Cell, human,
  Connector, Task, and Service nodes; there is no duplicate Workflow runtime.
- **FaaS — Function as a Service.** Functions owns immutable release/profile
  and invocation semantics. A Function runs as a Runtime <code>Task</code>,
  stateless <code>Service</code>, or external FaaS Connector. Sessionless MCP
  and calls from A3S Code use the same modes.
- **Durable Cell.** Durable Cells owns application revision, compatibility,
  retention, and deployment/storage correlation. An ordinary Runtime
  <code>Service</code> provides a named, serialized, hibernatable state space
  for people and multiple Agents without copying Agent or Workflow history.
- **Model Inference.** Inference owns model revision, deployment, role
  topology, routing policy, usage, and evaluation. It supports independent
  replicas and typed multi-node prefill/decode groups on the shared CPU/GPU
  placement rail.
- **Static Web.** Applications and Assets own immutable Web releases. React,
  Vue, and other admitted objects are served directly by Gateway with cache,
  CSP, SPA fallback, route, and rollback policy; SSR is an ordinary
  <code>Service</code> profile.

The only general execution classes are **Task** and **Service**. Agent,
Function, MCP, inference, Cell, build, and Cloud-system behavior is expressed
through immutable consumer-owned profiles. A3S Runtime owns the unified
lifecycle contract; A3S Box providers implement it.

## Quick start

### Requirements

- Rust 1.88 or later
- PostgreSQL 17 or a compatible supported release
- Git CLI for pinned external-source acquisition
- A3S Box for node-local workload/build execution
- the pinned A3S Gateway revision for routed services
- NATS JetStream for production <code>all</code>, Worker, or Relay roles
- Bun only for TypeScript client or CLI development

### Start the development API

~~~bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_POSTGRES_MIGRATION_URL="$A3S_CLOUD_POSTGRES_URL"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane --bin a3s-cloud-migrate -- config/cloud.acl
cargo run -p a3s-cloud-control-plane -- config/cloud.acl
~~~

Serving processes never run migrations. The one-shot migrator runs after
PostgreSQL is reachable and before API, Worker, Relay, or <code>all</code>;
production uses distinct migration and serving principals.

~~~bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
~~~

Direct port access is a local-development convenience. Production publishes
the API only through A3S Gateway.

<details>
<summary><strong>Bootstrap the first Organization</strong></summary>

Cloud stores only the API-token digest; the caller creates and retains the
credential. The request below also creates the accepted baseline platform-role
policy and binds <code>PlatformOwner</code> to the same bootstrap Principal.
Concurrent identical requests serialize before replay, and any policy, audit,
or Outbox failure rolls back the complete identity and authority root.

~~~bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: $A3S_CLOUD_BOOTSTRAP_TOKEN" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"$A3S_CLOUD_ADMIN_TOKEN\",\"expiresAt\":null}"
~~~

Subsequent mutations use <code>Authorization: Bearer ...</code> and a stable
<code>idempotency-key</code>.

</details>

### Use the CLI

~~~bash
bun install --cwd cli --frozen-lockfile

export A3S_CLOUD_TOKEN="$A3S_CLOUD_ADMIN_TOKEN"
export A3S_CLOUD_URL="http://127.0.0.1:8080/api/v1"

bun run --cwd cli src/main.ts diagnostics status --output=json
bun run --cwd cli src/main.ts organizations list --output=json
bun run --cwd cli src/main.ts operations list --output=json
~~~

Credentials come from environment variables or standard input and are never
written to a CLI context file.

## Platform capabilities

### Build, supply, and promote

- Hosted Git authority plus external source revisions, webhooks, reproducible
  Box builds, provenance, pull-request previews, immutable artifacts, and
  digest-preserving promotion.
- Separate **Git**, **OCI Registry**, and **A3S Use Registry** authorities.
  None is overloaded to impersonate another supply type.
- Governed logical Models and Model Revisions plus immutable model-weight
  manifests/objects, external hub resolution such as ModelScope, licenses,
  trust policy, and reconstructible node caches.
- One Flow-backed CI/CD model for Agent, Workflow, Function/MCP, Durable Cell,
  inference, Static Web, and Cloud system-service releases.

### Run, scale, and recover

- One heterogeneous scheduler for CPU pools, GPU pools, accelerator/topology
  constraints, Claims, anti-affinity, gang placement, maintenance, quotas, and
  preemption policy.
- Stateless horizontal scale and scale-to-zero; stateful drain, single-writer
  fencing, checkpoint/handoff, recovery, and locality-aware placement.
- Distributed inference with independent replicas, multi-node groups,
  prefill/decode-disaggregated roles, explicit KV transfer, and one shared
  scheduler rather than a second inference control plane.
- Independently scalable API, Worker, Relay, migrator, node-agent, and Gateway
  roles with explicit readiness, migration, lease/leader, rollout, and recovery
  contracts.

### Store, serve, and observe

- One typed immutable-object client over external HTTPS AWS S3 or
  S3-compatible storage. Cloud bundles no S3 server and does not present object
  storage as POSIX/FUSE. Mutable volumes, backup, restore, retention, and writer
  fencing belong to Data/S0.
- A3S Gateway owns TLS, authentication, request limits, routes, model/Agent/
  Function/MCP endpoints, and tenant Web delivery. Cloud services stay private.
- OpenTelemetry correlates logs, metrics, traces, SLOs, and incidents.
  Immutable evidence stays with domain owners; Apache Doris is an optional,
  rebuildable analytics and SLO projection.

### Govern tenants and privileged access

- One immutable Installation identity and one discriminated
  Installation/Organization/Project/Environment scope contract across tenant
  isolation, memberships, Resource Grants, quotas, credentials, audit, Outbox,
  and lifecycle cleanup. Platform facts never borrow a sentinel Organization.
- A distinct system-administrator RBAC plane for installation, fleet,
  migration, policy, incident, and break-glass duties. A system role never
  silently grants tenant data or Secret access.
- Tenant support requires an active exact human, an admitted support-use role,
  a short-lived non-renewing grant, descendant scope, and one closed
  non-sensitive permission. Each allow pins replayable policy, credential,
  binding, grant, action, resource, and request evidence.
- Fresh bootstrap atomically creates the first Organization, service Principal,
  owner Membership, API token, accepted baseline platform-role policy, and
  matching <code>PlatformOwner</code> binding with shared audit, Outbox, and
  idempotency facts.
- Privileged mutations and installation-wide organization-catalog reads use
  the same Identity/PostgreSQL decision issuer. A valid exact
  <code>cloud:read</code> credential without
  <code>TenantLifecycleRead</code> sees only its own Organization; revoked,
  expired, mismatched, or under-scoped credentials fail closed.
- Workload trust uses immutable TrustDomain and WorkloadIdentityPolicy
  revisions in the same Identity authority. Current, exact, bounded-history,
  and workload-indexed reads plus CAS-fenced acceptance are exposed through
  REST/OpenAPI, the TypeScript client, and CLI without caller-authored actor,
  credential, or Installation overrides.
- A canonical <code>cloud.identity.workload-provider.v1</code> profile binds
  each TrustDomain revision to one replaceable provider adapter by digest. The
  API-only <code>spiffe_https_web</code> adapter performs a fresh HTTPS,
  bounded, strict-JSON SPIFFE bundle observation and admits it against that
  exact revision. Its contract labels endpoint evidence as <code>observed*</code>
  and digest-bound profile policy as <code>declared*</code>; it owns no
  certificate issuance, private key, provider registry, or parallel trust
  state.
- The versioned
  <code>cloud.identity.workload-runtime-evidence-binding.v1</code> foundation
  binds one exact policy digest to its Workloads Claim, NodePool and Fleet Node
  session/capability snapshot, plus Runtime Unit generation and Box provider
  attestation. Verified C2 composes only the Workloads and Fleet owner facts.
  Component-only C3a admits one generic Identity authorization before
  scheduling and persists an immutable Workloads record, including an explicit
  no-policy outcome, so crash replay cannot relabel a legacy or running Unit.
  The deterministic V1 evidence still lacks Node hardware attestation and
  cannot authorize credential issuance; that fail-closed gap remains a required
  WI2 gate rather than an inferred capability.
- OpenShift-class outcomes—reconciliation, scheduling, isolation, rollout,
  policy, observability, and day-two operations—and TokenHub-class
  outcomes—governed model/provider/key access, routing, quotas, diagnostics,
  and usage—are composed through A3S authorities rather than copied APIs or
  control planes.

## Consistency by construction

| Concern | Canonical rule |
| --- | --- |
| Command concurrency | Exact tenant scope, idempotency key, expected version, and payload digest are checked transactionally; drift or conflicting replay fails closed |
| Database writes | Aggregate, idempotency, audit, and Outbox commit together through A3S ORM/PostgreSQL; the database resolves canonical Installation lineage |
| Cross-system work | A3S Flow sagas and owner receipts reconcile uncertain outcomes; no database transaction spans an external provider |
| Rate limits and quotas | Gateway enforces request limits; owner admission enforces durable quota. Redis may accelerate counters but never becomes quota truth |
| Cache | Redis holds bounded, reconstructible reads, discovery, tokens, and coordination hints with revisioned invalidation; cache loss changes latency, not correctness |
| Locks and leases | PostgreSQL/CAS owns correctness and fencing. A distributed lock may reduce contention but cannot replace versions or Fleet Claims |
| Dispatch pressure | A3S Lane admits only post-durable work for fairness, backpressure, and bounded concurrency; it owns neither workflow nor queue truth |
| Analytics | Doris consumes reconstructible telemetry/evidence projections; PostgreSQL and bounded-context owners remain operational truth |

## DDD and single authority

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="A3S Cloud DDD dependency direction from inbound adapters through Presentation, Application, Domain, inward-owned ports, Infrastructure providers, and committed integration facts" />
</p>

Presentation calls Application; Application coordinates its Domain and
consumer-owned ports; Infrastructure implements those inward ports. Contexts
collaborate only through a synchronous owner Application contract or a
versioned fact emitted from the owner's committed Outbox.

| Concern | Sole authority | Forbidden duplicate |
| --- | --- | --- |
| Tenant identity and authorization | Identity + Projects | Adapter-local roles, UI-only policy, provider sessions, or cache claims as truth |
| Product meaning | Owning Agent, Workflow, Function, Cell, Inference, Application, or Asset context | Runtime/provider fields becoming product state |
| Durable coordination | Operations + A3S Flow | Product retry tables, sleep loops, or another workflow engine |
| Build and release delivery | Sources + Artifacts + product Release owner + Delivery Pipelines | Product-local CI state, rebuild-on-promotion, or mutable deployment tags |
| Placement and rollout | Workloads + Fleet | Agent-, MCP-, Cell-, model-, or Gateway-specific schedulers |
| Provider lifecycle | A3S Runtime + A3S Box | Direct process/container/FaaS calls from product domains |
| Public traffic | Edge desired state + A3S Gateway applied state | Cloud proxy, per-product ingress, or another Gateway publisher |
| Immutable and mutable data | Shared object client + Data/S0 | Per-product S3 clients, backup engines, or provider state as desired-state truth |
| Integration facts | One scope-aware transactional Outbox + A3S Event | Publish-before-commit, sentinel tenants, or parallel product/platform event buses |
| Configuration | A3S ACL parsed by <code>a3s-acl</code> | Compatibility parsers or another product configuration language |

Cross-cutting behavior follows one visible ordered pipeline: authentication,
authorization, validation, idempotency, transaction, audit/Outbox, then
dispatch. Logging, tracing, metrics, cache, rate limits, and AOP interceptors
observe or protect that path; none may become a second business authority.
[Executable architecture ratchets](docs/architecture-audit.md) stop outer-layer
imports and duplicate mechanisms from spreading while known debt is removed.

## Delivery status

The portfolio is gate-driven, not percentage-driven. As of **2026-08-30**:

| Lane | Evidence state |
| --- | --- |
| Tenant-scoped Identity, PostgreSQL/A3S ORM, Operations/Flow, Outbox, public API, and migrations | **Verified foundation** |
| Installation scope and system-administrator RBAC | **Verified core, broader gate in progress.** Atomic fresh bootstrap, policy/binding and support-grant repositories, exact privileged decisions, protected mutations, REST/OpenAPI, TypeScript client, CLI, Management MCP, and revocation-fenced organization catalog are verified. Controlled recovery for pre-root installations plus the wider MT3 role matrix, owner-port cleanup, and hostile-tenant evidence remain |
| Workloads, Fleet, Runtime/Box, Gateway, supply, collaboration, and enterprise controls | **In progress.** Several component/provider gates exist; complete release recertification remains |
| Agent and hosted MCP product lanes | **In progress.** Component evidence does not imply complete AaaS availability |
| Ontology Workflow and AI Applications/Files | **In progress.** Complete WaaS and Application products remain gate-bound |
| Data/S0 and Durable Cell | **Foundation in progress.** Durable Cell is a first-class target but not yet an available managed service |
| Workload identity | **Verified trust and WI2-C1/C2 foundation; C3a component implemented locally.** The [trust/provider main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33291073009), [C1/C2 main CI](https://github.com/A3S-Lab/Cloud/actions/runs/33310808529), and [Box provider conformance](https://github.com/A3S-Lab/Cloud/actions/runs/33310808538) pass. C3a now production-composes the generic Identity authorization ACL and one immutable Workloads pre-scheduling bound/no-policy record through migration `180`; current ordinary, placement-group and reconciliation paths share it, while legacy Deployments remain unmodified. This slice still awaits main verification, and Identity evidence history, Fleet hardware attestation, issuance, enforcement, revocation, and federation remain open |
| FaaS, distributed inference, model supply, Static Web, Runtime CI/CD, and full HA operations | **Planned or early foundation.** Their architecture and authority boundaries are defined, but complete product gates remain |

See the [product roadmap](ROADMAP.md), [platform gap
analysis](docs/platform-gap-analysis.md), and [ecosystem project
roadmaps](docs/project-roadmaps/README.md) for exact dependencies, evidence, and
remaining gates.

## Deployment model

Cloud system services and tenant workloads share mechanisms but never borrow
authority:

- the bootstrap plane installs PostgreSQL, NATS, S3-compatible storage, Git,
  OCI Registry, A3S Use Registry, migrator, API, Worker, Relay, Gateway, and
  observability dependencies through one dependency DAG;
- API, Worker, Relay, migrator, node agent, and Gateway scale independently;
- tenant workloads enter only through admitted releases, Workloads/Fleet
  placement, Runtime/Box execution, and Gateway publication;
- management is API/OpenAPI/client/CLI/MCP-first; there is no Cloud Dashboard
  or UI-specific backend.

The initial Box-hosted profile is the installation foundation. Production HA
requires the named clean-install, upgrade, rollback, dependency-loss,
credential-rotation, storage-recovery, node-drain, and multi-replica gates in
the [deployment architecture](docs/deployment-and-cluster-architecture.md).

## Interfaces and configuration

| Surface | Contract | Start here |
| --- | --- | --- |
| REST/OpenAPI | Versioned <code>/api/v1</code>, request IDs, idempotency, common envelopes, committed snapshot | [Guide](docs/openapi.md) · [openapi/v1.json](openapi/v1.json) |
| TypeScript client | Maintained adapter over the same REST contract | [packages/cloud-client](packages/cloud-client) |
| CLI | Scriptable structured output with no token argument | [cli/README.md](cli/README.md) |
| Management MCP | Sessionless, tenant-authorized tools over the same Application handlers | [docs/management-mcp.md](docs/management-mcp.md) |

Cloud and the Node Agent accept only closed, validated **A3S ACL** parsed by
<code>a3s-acl</code>. Unknown fields and unsafe timing relationships fail before
startup; Secret values never belong in ACL. Start with
[config/cloud.acl](config/cloud.acl),
[config/node.example.acl](config/node.example.acl), and the
[deploy/production](deploy/production/README.md) baseline.

Redis is optional acceleration, Doris is optional analytics, and neither is
durable truth. S3-compatible object storage and NATS are external production
dependencies.

## Repository and development

<details>
<summary><strong>Repository layout</strong></summary>

~~~text
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
~~~

</details>

<details>
<summary><strong>Core development gates</strong></summary>

~~~bash
cargo fmt --all -- --check
cargo test -p a3s-cloud-control-plane architecture_tests --lib
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
~~~

Real-provider and release certification runs on isolated Linux hosts. Important
repository-owned gates include [cross-surface
conformance](tools/c0-conformance/README.md), [Runtime
conformance](tools/runtime-conformance/README.md), [Box provider
conformance](tools/box-conformance/README.md), [workload-identity provider
conformance](tools/workload-identity-conformance/README.md), and the [pinned
Gateway revision](tools/gateway-conformance/gateway-revision).

</details>

## Documentation

| Start here | Purpose |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, evidence, and delivery order |
| [Technical architecture](docs/architecture.md) | Stable ownership, topology, consistency, and failure behavior |
| [AI service platform](docs/ai-service-platform-architecture.md) | AaaS, WaaS, FaaS, Durable Cell, Inference, Runtime, Box, and Gateway composition |
| [Agent Runtime](docs/agent-runtime-architecture.md) · [Function Runtime](docs/function-runtime-architecture.md) · [Durable Cell](docs/durable-cell-platform-plan.md) | Service semantics over the unified Runtime |
| [Static Web](docs/static-web-hosting-architecture.md) · [model supply](docs/model-supply-architecture.md) · [inference](docs/inference-plan.md) | Tenant UI, models/weights, and serving architecture |
| [Cluster deployment](docs/deployment-and-cluster-architecture.md) · [elastic services](docs/elastic-service-deployment-architecture.md) | System services, CPU/GPU scheduling, stateful/stateless convergence, and HA |
| [Runtime CI/CD](docs/runtime-cicd-architecture.md) · [workload identity](docs/workload-identity-and-service-connectivity-architecture.md) | Delivery, attestation, private discovery, mTLS, and revocation |
| [Distributed API consistency](docs/distributed-api-consistency-architecture.md) · [Redis and Lane](docs/redis-and-lane-platform-architecture.md) | Concurrency, transactions, cache, locks, fairness, and backpressure |
| [Observability and analytics](docs/observability-and-analytics-architecture.md) · [platform gap analysis](docs/platform-gap-analysis.md) | Telemetry/SLO/incident design and prioritized missing outcomes |
| [Multi-tenant platform](docs/multi-tenant-developer-platform-architecture.md) · [capability architecture](docs/platform-capability-architecture.md) | Tenant/admin RBAC and OpenShift-/TokenHub-class outcomes |
| [DDD, AOP, and patterns](docs/ddd-aop-and-pattern-architecture.md) · [architecture audit](docs/architecture-audit.md) | Layer rules, aspect order, patterns, and executable debt ratchets |
| [Ecosystem roadmaps](docs/project-roadmaps/README.md) | Mission, dependencies, evidence, and negative boundary for every A3S subproject |

## License

[MIT](LICENSE)
