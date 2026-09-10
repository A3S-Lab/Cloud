# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud turns Agent, Workflow, Function, Durable Cell, inference, and Web semantics into governed services on A3S Runtime and Box" />
</p>

<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=release" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.87.0" src="https://img.shields.io/badge/REST_contract-1.87.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#what-cloud-is">First principles</a> &middot;
  <a href="#how-it-works">Architecture</a> &middot;
  <a href="#product-lanes">Product lanes</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#delivery-status">Delivery</a> &middot;
  <a href="#documentation">Docs</a>
</p>

**A3S Cloud is a self-hosted control plane that turns tenant-authorized product
intent into desired state on operator-owned CPU/GPU infrastructure.** Cloud owns
product and desired-state truth in PostgreSQL. A3S Flow coordinates durable work.
A3S Runtime defines one lifecycle contract. A3S Box executes it. A3S Gateway is
the only public edge. Product lanes (AaaS, WaaS, FaaS, Durable Cell, inference,
Static Web) are projections over that single path—not six runtimes.

> [!IMPORTANT]
> Architecture targets are not availability claims. A capability is released
> only after its real-provider, failure, recovery, cleanup, upgrade, and release
> gates are marked <code>Verified</code> in [ROADMAP.md](ROADMAP.md). Until then,
> treat the lane as unavailable even if components exist.

> [!NOTE]
> A3S Cloud does not ship a management Dashboard. Management is
> REST/OpenAPI, TypeScript client, CLI, and Management MCP. Tenant Web releases
> (when <code>WEB0</code> lands) use the same Gateway and APIs as every other
> client.

> [!TIP]
> Automatic CI and Box conformance run only for pushes to <code>release</code>
> and pull requests targeting <code>release</code>. <code>main</code> does not
> start those workflows automatically; use workflow dispatch for an ad hoc run.

## What Cloud is

First principles for reading this repository:

| Cloud is | Cloud is not |
| --- | --- |
| Desired-state and product authority in PostgreSQL | A second request-byte proxy beside Gateway |
| One Workloads + Fleet placement path for every service class | Per-product schedulers (Agent / MCP / Cell / GPU / inference) |
| Edge compiler of complete Gateway snapshots | Bearer-secret store for inference keys (Identity owns issuance; Edge projects ACL only) |
| Outbox → Event integration after commit | Publish-before-commit or UI-only policy |
| API / CLI / MCP-first management | A Cloud Dashboard backend |
| Gate-driven delivery | “Feature-complete because the module folder exists” |

**Truth hierarchy:** PostgreSQL and owning bounded contexts are operational
truth. Redis may accelerate. Doris may project analytics. Neither becomes
quota, grant, or desired-state authority.

## How it works

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud authority map from A3S Gateway through tenant product domains, Identity, PostgreSQL, Flow, Workloads and Fleet, Runtime, Box, supply, storage, dispatch, and observability" />
</p>

Every admitted service follows one path:

1. **Admit.** Gateway authenticates public traffic; Identity resolves
   Installation, Organization, Project, Environment, Principal, and credential
   scope.
2. **Commit.** An Application use case atomically writes desired state,
   idempotency, audit, and Outbox facts to PostgreSQL through A3S ORM.
3. **Coordinate.** Operations and A3S Flow own durable waits, retry, replay,
   approval, compensation, cancellation, and delivery history.
4. **Place and execute.** Workloads and Fleet own Claims, rollout, drain, and
   fencing; the Node Agent converges Runtime <code>Task</code> or
   <code>Service</code> units through A3S Box.
5. **Publish.** Edge compiles a complete versioned route snapshot; Fleet
   delivers it; the Node Agent installs it; Gateway applies and serves it.

Runtime CI/CD uses the same map: build once, verify the digest, promote the
same immutable release, deploy through Workloads/Fleet, shift traffic through
Edge/Gateway, and roll back to an earlier admitted release.

### Control and data planes

| Plane | Authority | Notes |
| --- | --- | --- |
| Tenant / admin API | Control-plane <code>api</code> role | Direct port access is local-dev only; production publishes through Gateway |
| Durable workers | <code>worker</code> role | Health + reconcilers; no management surface |
| Outbox relay | <code>relay</code> role | Event publication only |
| Node control | Fleet ↔ Node Agent (outbound mTLS) | Commands, observations, Gateway snapshot install—separate from tenant request bytes |
| Public traffic | Edge desired state → Gateway applied state | Cloud never proxies request bodies |
| Inference ACL (partial) | Identity credentials + Inference routes → Edge compiler | Workers stay empty until Power observation delivery (<code>PW0</code>); full OpenAI data plane waits on <code>BX0</code> + <code>PW0</code> |

## Product lanes

Six product outcomes share two execution classes (**Task** and **Service**).
Status vocabulary matches [ROADMAP.md](ROADMAP.md): **Verified**, **In
progress**, **Planned**.

| Lane | Owner intent | Status |
| --- | --- | --- |
| **AaaS** — Agent as a Service | Conversations, executions, events, approvals, checkpoints, Tool evidence | **In progress** (A0.4 / A1.0 / A1.2 verified; complete AaaS still gate-bound) |
| **WaaS** — Workflow as a Service | Ontology, immutable plans, WorkflowRun, HumanTask; Flow coordinates nodes | **In progress** (unavailable as a complete product) |
| **FaaS** — Function as a Service | Immutable Function profile; invoke via Executions / Workloads / Connectors | **In progress / unavailable** (<code>FN0.1</code> contracts frozen) |
| **Durable Cell** | Named, serialized, hibernatable shared state over ordinary Service | **In progress / unavailable** |
| **Model inference** | Keys, route catalog, Edge ACL, usage ledger; Power serving on Box | **Planned** overall (<code>I0</code>); Cloud-side keys/routes/Edge/usage components exist; end-to-end serving blocked by <code>BX0</code> + <code>PW0</code> |
| **Static Web** | Immutable Web releases served by Gateway | **Planned** (<code>WEB0</code>; Gateway static-object target not implemented) |

Platform foundations that already carry product work:

| Foundation | Status |
| --- | --- |
| Control plane, PostgreSQL/ORM, Operations/Flow, Outbox, migrations, public API | **Verified** (<code>F0</code>) |
| REST / TypeScript client / CLI / Management MCP parity | **Verified** core (<code>C0.1</code>–<code>C0.2m</code>) |
| Workloads replicas + Gateway target projection | **Verified** (<code>H0.1</code>–<code>H0.2</code>) |
| Box-only execution/build re-certification | **In progress** (<code>BX0</code>; release blocker) |
| A3S Power as Box-hosted inference Service | **Planned** (<code>PW0</code>) |

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

From the A3S monorepo root (preferred one-click path):

~~~bash
just up          # deps (a3s-box compose, or Docker fallback) + control-plane detached
curl http://127.0.0.1:8080/api/v1/health/live
just down        # stop API and local compose/docker deps
~~~

State, logs, and the generated bootstrap token live under
<code>apps/cloud/.a3s/cloud/dev/</code>. Prefer <code>a3s-box compose</code>
with [deploy/dev/compose.acl](deploy/dev/compose.acl); when Box is unavailable,
<code>just up</code> falls back to Docker on the same ports and bootstraps the
migration/serving Postgres roles.

Inside this repository alone:

~~~bash
just up
# or foreground: just cloud
~~~

Manual env (production-shaped principals):

~~~bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud_serving:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_POSTGRES_MIGRATION_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
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

<details>
<summary><strong>Bootstrap the first Organization</strong></summary>

Cloud stores only the API-token digest; the caller creates and retains the
credential. The request also creates the accepted baseline platform-role policy
and binds <code>PlatformOwner</code> to the same bootstrap Principal.

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

## Consistency by construction

| Concern | Canonical rule |
| --- | --- |
| Command concurrency | Exact tenant scope, idempotency key, expected version, and payload digest are checked transactionally; drift or conflicting replay fails closed |
| Database writes | Aggregate, idempotency, audit, and Outbox commit together through A3S ORM/PostgreSQL |
| Cross-system work | A3S Flow sagas and owner receipts reconcile uncertain outcomes; no database transaction spans an external provider |
| Rate limits and quotas | Gateway enforces request limits; owner admission enforces durable quota. Redis never becomes quota truth |
| Cache | Redis holds bounded, reconstructible hints; cache loss changes latency, not correctness |
| Locks and leases | PostgreSQL/CAS owns fencing. A distributed lock may reduce contention but cannot replace versions or Fleet Claims |
| Dispatch pressure | A3S Lane admits only post-durable work; it owns neither workflow nor queue truth |
| Analytics | Doris consumes reconstructible projections; PostgreSQL and bounded-context owners remain operational truth |

## DDD and single authority

<p align="center">
  <img src="assets/readme/ddd-boundary.svg" width="100%" alt="A3S Cloud DDD dependency direction from inbound adapters through Presentation, Application, Domain, inward-owned ports, Infrastructure providers, and committed integration facts" />
</p>

Presentation calls Application; Application coordinates its Domain and
consumer-owned ports; Infrastructure implements those ports. Contexts
collaborate only through a synchronous owner Application contract or a
versioned fact from the owner's committed Outbox.

| Concern | Sole authority | Forbidden duplicate |
| --- | --- | --- |
| Tenant identity and authorization | Identity + Projects | Adapter-local roles, UI-only policy, cache-as-truth |
| Product meaning | Owning Agent, Workflow, Function, Cell, Inference, Application, or Asset context | Runtime/provider fields as product state |
| Durable coordination | Operations + A3S Flow | Product retry tables or another workflow engine |
| Placement and rollout | Workloads + Fleet | Agent-, MCP-, Cell-, model-, or Gateway-specific schedulers |
| Provider lifecycle | A3S Runtime + A3S Box | Direct process/container/FaaS calls from product domains |
| Public traffic | Edge desired state + Gateway applied state | Cloud proxy or parallel Gateway publishers |
| Immutable / mutable data | Shared object client + Data/S0 | Per-product S3 clients as desired-state truth |
| Integration facts | Scope-aware transactional Outbox + A3S Event | Publish-before-commit |
| Configuration | A3S ACL via <code>a3s-acl</code> | Compatibility parsers or another config language |

Cross-cutting order is fixed: authentication → authorization → validation →
idempotency → transaction → audit/Outbox → dispatch. Observability and rate
limits protect that path; none becomes a second business authority.
[Executable architecture ratchets](docs/architecture-audit.md) keep outer-layer
imports and duplicate mechanisms from spreading.

## Delivery status

Gate-driven, not percentage-driven. Summary as of **2026-09-10** (exact
evidence and remaining exits live in [ROADMAP.md](ROADMAP.md)):

| Area | Evidence state |
| --- | --- |
| Foundation (<code>F0</code>): Identity, PostgreSQL/ORM, Flow/Operations, Outbox, API, migrations | **Verified** |
| Control surfaces (<code>C0.1</code>–<code>C0.2m</code>) | **Verified** core; enterprise <code>C0.5</code> / broader <code>C0.3</code> slices still open |
| Workloads / Fleet / Gateway projection (<code>H0.1</code>–<code>H0.2</code>) | **Verified**; multi-node HA / autoscaling (<code>H0.3</code>+) in progress |
| Box-only platform (<code>BX0</code>) | **In progress** (release blocker for Box-backed production claims) |
| Agent lanes (<code>A0</code>/<code>A1</code>) | **In progress**; A0.4 and selected A1 gates verified—complete AaaS still gate-bound |
| Workflow / Applications / Automations / Cells / Knowledge | **In progress / unavailable** as complete products |
| Inference (<code>I0</code>) | **Planned** product; Cloud keys, route catalog, Edge ACL succession, and usage ledger components exist; Power workers and end-to-end OpenAI data plane wait on <code>BX0</code> + <code>PW0</code> |
| FaaS / Static Web / Runtime CI/CD / Power | **Planned** or early foundation |

## Deployment model

- Bootstrap installs PostgreSQL, NATS, S3-compatible storage, Git, OCI Registry,
  A3S Use Registry, migrator, API, Worker, Relay, Gateway, and observability
  dependencies through one dependency DAG.
- API, Worker, Relay, migrator, Node Agent, and Gateway scale independently.
- Tenant workloads enter only through admitted releases, Workloads/Fleet
  placement, Runtime/Box execution, and Gateway publication.
- Production HA requires the named clean-install, upgrade, rollback,
  dependency-loss, credential-rotation, storage-recovery, node-drain, and
  multi-replica gates in
  [deployment architecture](docs/deployment-and-cluster-architecture.md).

## Interfaces and configuration

| Surface | Contract | Start here |
| --- | --- | --- |
| REST/OpenAPI | Versioned <code>/api/v1</code>, request IDs, idempotency, common envelopes | [Guide](docs/openapi.md) · [openapi/v1.json](openapi/v1.json) |
| TypeScript client | Maintained adapter over the same REST contract | [packages/cloud-client](packages/cloud-client) |
| CLI | Scriptable structured output with no token argument | [cli/README.md](cli/README.md) |
| Management MCP | Sessionless, tenant-authorized tools over the same Application handlers | [docs/management-mcp.md](docs/management-mcp.md) |

Cloud and the Node Agent accept only closed, validated **A3S ACL** parsed by
<code>a3s-acl</code>. Unknown fields and unsafe timing fail before startup;
Secret values never belong in ACL. Start with
[config/cloud.acl](config/cloud.acl),
[config/node.example.acl](config/node.example.acl), and
[deploy/production](deploy/production/README.md).

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

Real-provider and release certification runs on isolated Linux hosts. Repository
gates include [cross-surface conformance](tools/c0-conformance/README.md),
[Runtime conformance](tools/runtime-conformance/README.md),
[Box provider conformance](tools/box-conformance/README.md),
[workload-identity conformance](tools/workload-identity-conformance/README.md),
and the [pinned Gateway revision](tools/gateway-conformance/gateway-revision).

</details>

## Documentation

| Start here | Purpose |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, evidence, delivery order |
| [Architecture optimization roadmap](docs/architecture-optimization-roadmap.md) | Execution waves, dual-track I0, integrity track |
| [Ecosystem project roadmaps](docs/project-roadmaps/README.md) | Per-subproject Cloud obligations and portfolio waves |
| [Technical architecture](docs/architecture.md) | Ownership, topology, consistency, failure behavior |
| [AI service platform](docs/ai-service-platform-architecture.md) | AaaS / WaaS / FaaS / Cell / Inference composition |
| [Inference plan](docs/inference-plan.md) | I0 contracts, Edge ACL, usage, Power/Box dependencies |
| [Cluster deployment](docs/deployment-and-cluster-architecture.md) | System services, scheduling, HA |
| [Workload identity](docs/workload-identity-and-service-connectivity-architecture.md) | Trust, attestation, private discovery |
| [Capability architecture](docs/platform-capability-architecture.md) | OpenShift-/TokenHub-class outcomes without copied APIs |
| [Architecture audit](docs/architecture-audit.md) | Executable debt ratchets |
| [Ecosystem roadmaps](docs/project-roadmaps/README.md) | Mission and negative boundary per A3S subproject |

## License

[MIT](LICENSE)
