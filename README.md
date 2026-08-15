# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud turns authorized intent into exact applied state through one durable control loop" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.37.0" src="https://img.shields.io/badge/REST_contract-1.37.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#product-system">Products</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#current-delivery">Delivery</a> &middot;
  <a href="#quick-start">Quick start</a> &middot;
  <a href="#management-surfaces">Interfaces</a> &middot;
  <a href="#documentation">Documentation</a>
</p>

**A3S Cloud is a self-hosted control plane for operating AI applications,
Knowledge Pipelines, Agents, MCP servers, Workflows, automations, and model
services on infrastructure you own.** It turns authorized tenant intent into
versioned, exact applied state through one PostgreSQL authority, one durable
control loop, one outbound node channel, and one provider-neutral execution
path.

> [!IMPORTANT]
> This README describes the stable product architecture and the implemented
> backend foundation. It is not a blanket availability claim. Exact public
> gates, provider evidence, and remaining work live in
> [ROADMAP.md](ROADMAP.md).

## Product system

Five products compose the same Cloud authorities. None creates its own control
plane, scheduler, runtime, queue, identity store, or evidence rail.

| Product | Outcome | Shared foundation |
| --- | --- | --- |
| **01 / Unified Gateway** | One governed entry for Workflow, Agent, MCP, model, and application traffic | Identity and Edge own policy; Fleet delivers; A3S Gateway alone applies the live byte path |
| **02 / Workflow Orchestration** | Compile ontology-defined goals and typed graphs into recoverable execution | Workflow owns immutable semantics; A3S Flow and Operations own durable orchestration |
| **03 / Agent Factory** | Turn heterogeneous Harness implementations into immutable, evaluated, deployable Agent products | Assets, Agents, Workloads, Fleet, Runtime, Box, and one `AgentExecutionProvider` boundary |
| **04 / AI Application Platform** | Build, publish, monitor, and govern application experiences, Knowledge, plugins, and automations | Applications compose exact Workflow and Agent revisions with existing platform owners |
| **05 / Durable Cell Service** | Run named, SQLite-backed state entities with alarms, WebSockets, idle eviction, and fenced recovery | Durable Cells owns application intent; Workloads/Fleet host one ordinary Runtime Service fleet; the selected provider and S0 own per-Cell state and fencing |

A3S Code is one first-party Harness provider, not a privileged execution path.
Security operations correlate Gateway, Runtime, Box, Agent, A3S Sentry,
AnySentry, and audit evidence without creating a second security control plane.

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud architecture with one PostgreSQL control authority, one outbound node execution path, and a separate Gateway request path" />
</p>

The modular monolith runs `api`, `worker`, and `relay` roles together or
separately from the same binary. PostgreSQL remains business authority in every
profile. A3S Event accelerates committed facts but never replaces recovery
scans.

The architecture separates three paths:

1. **Control:** accepted mutations atomically commit desired state,
   idempotency, an Operation, audit, and bounded Outbox facts.
2. **Execution:** Workloads and Fleet reserve exact Claims and deliver one
   versioned command through the outbound-only Node Agent. Runtime owns Task
   and Service lifecycle; Box is the sole local execution/build provider.
3. **Live requests:** A3S Gateway sends opaque bytes directly to an exact
   healthy workload, Harness, MCP, Power, or Durable Cell provider endpoint.
   Cloud stays off this byte path and advances only from matching applied
   evidence.

Explore the [interactive architecture](https://a3s-lab.github.io/Cloud/architecture/)
or read the [technical architecture](docs/architecture.md) for bounded
contexts, consistency boundaries, failure behavior, and the full
capability-preservation register.

### One Gateway publication path

<p align="center">
  <img src="assets/readme/gateway-publication.svg" width="100%" alt="Ordinary Routes and hosted MCP scopes enter one desired-state planner, complete snapshot compiler, atomic publication owner, Fleet command, and exact acknowledgement path" />
</p>

Ordinary Route and hosted MCP changes reread the complete node-scoped desired
state before compiling. Atomic staging records the exact scope-set CAS,
snapshot revision, command, and one durable publication owner. Both sources
reuse the same Fleet command and acknowledgement projection, so neither can
erase the other's routes or dispatch the same snapshot twice.

This is an implemented orchestration foundation, not a hosted-MCP availability
claim. The joint Runtime, Box, Gateway, recovery, and cleanup gates still apply.

### One concern, one authority

| Concern | Sole authority | Deliberately absent duplicate |
| --- | --- | --- |
| Business desired state | PostgreSQL through A3S ORM | Redis, stream, node journal, or local file as product truth |
| Workflow semantics | Immutable Workflow/Ontology revisions and exact Plans | Flow history as business schema, mutable authoring payloads, or another graph engine |
| Composite-region policy | Immutable WorkflowRevision semantic child; exact child revision in the graph | Cloud-owned scheduler, loop worker, queue, or state store |
| Long-running execution | A3S Flow plus Cloud Operations | Product-specific workflow engines and retry loops |
| Placement and rollout | Workloads | Agent-, MCP-, inference-, or Gateway-specific schedulers |
| Node delivery and hard resources | Fleet, Node Agent journal, and Claims | Direct process control or a second node channel |
| Provider lifecycle | A3S Runtime Task and Service | Product policy inside Runtime or direct provider calls from business contexts |
| Local execution and builds | A3S Box | Parallel Docker, BuildKit, sandbox, or Cloud executor paths |
| Durable Cell application intent | Durable Cells context plus immutable `cloud.durable-cell.service.v1` ACL | A second Workload controller, Runtime class, provider configuration store, or per-Cell Cloud scheduler |
| Per-Cell state and ownership | Selected Cell provider in one S0 application namespace | PostgreSQL Cell/lease/epoch/SQLite mirrors, Gateway owner lookup, or Cloud peer membership |
| Traffic application | Edge planner/compiler, Fleet command, A3S Gateway applied state | Ordinary/MCP publishers, Cloud proxying, or inferred success |
| Identity and authorization | Principals, Memberships, invitations, grants, credentials, and revocation | Console-local users, credential-owned roles, or adapter-specific RBAC |
| Personal and outbound notifications | Notifications owns the exact-recipient inbox and deterministic delivery intent; A3S Event owns durable consumption; Connectors and Secrets own targets and credentials; Identity owns verified contacts | A second event rail, provider-local retry scheduler, copied target/Secret/contact authority, or presentation-local inbox |
| Plugin package lifecycle | Cloud tenant intent plus the shared A3S Use Plugin Manager | Cloud installer, catalog copy, or parallel assignment store |
| Management behavior | One command/query application layer | REST-, CLI-, MCP-, or Web-specific business rules |

## Current delivery

Cloud is backend-first and gate-driven. Implemented does not automatically mean
public: a capability becomes available only after its required real-provider,
failure, recovery, cleanup, and release evidence passes.

| Area | Current foundation | Availability boundary |
| --- | --- | --- |
| Durable control | A3S Flow `0.12.0`, Boot `0.2.0`, ORM `0.3.0`, PostgreSQL queue, Operations, Outbox, and replay | `F0` verified |
| Management | REST/OpenAPI `1.37.0`, maintained TypeScript client, CLI, Management MCP, retained Web projection | Broader enterprise `C0` gates remain |
| Identity | Principals, Memberships, invitations, grants, tokens, OIDC link/login flows, audit, project attribution, in-app notifications, immutable personal outbound-subscription ACLs with non-Web management surfaces, transactional delivery facts, fixed provider-attempt termination, and monotonic terminal receipts around the fenced Connector path | User-configured suppression/delivery budgets, SMTP, retained production evidence, the intentionally deferred Web surface, and broader enterprise surfaces remain |
| Compute and delivery | Immutable sources/assets, builds, Executions, Workloads, Fleet, Node Agent, Edge snapshot publication, Gateway apply | Box-only recertification and clean-host provider gates remain |
| Workflow | Ontologies, immutable definitions/revisions/goals, Plan v2, WorkflowRun, Forms/HumanTasks, finite Execution, typed variables/defaults, inspection, node discovery, immutable composite-region policy, and a versioned per-step provider retry budget | Public Workflow, composite/Connector execution, remaining providers, compensation, and production evidence remain |
| Plugins | Exact A3S Use compatibility plus trusted Registry/catalog reads | Tenant assignments and complete `U0` gate remain |
| Agent execution | Provider-neutral Harness boundary and common workload path | Native Code verification and later governance gates remain |
| Connectors | Exact-revision profiles, canonical A3S ACL admission, authorized just-in-time Secret materialization, public-Internet DNS/SSRF enforcement with exact address pinning, durable pre-dispatch attempt fencing, one-shot authorized execution composition, atomic immutable terminal evidence, authorized bounded recovery reads, REST/OpenAPI/client/CLI/Management MCP profile lifecycle, the first Notification-owned A3S Event consumer composition, a component-only Workflow exact-attempt adapter over the same C6 service, and an immutable Workflow policy v2 retry budget | Workflow Flow scheduling/interpretation and immutable response-object composition, general provider wiring, revocation/recovery operations, retained integration evidence, and `AUT0.5` availability remain |
| Durable Cells | `CELL0.1-C1/C2/C3` freeze the application foundation; component-only `CELL0.2-C1/C2` reuse the sole object client and Secrets authority for CAS, exact credentials, sealed recovery/restore, retention/deletion, and exact storage correlation; the `C3` HTTPS S3-compatible CAS gate is checked in over the same shared client fixture; `CELL0.3-C1/C2` bind a digest-pinned provider to the existing Workload/Runtime Service projection, add a typed and Cell-name-free operator observation over Fleet's sole journal, and admit adoption/drain/cleanup only from exact existing Runtime receipts | A retained real-provider S0 pass, recovery/deletion execution, real Runtime/Box provider certification, orchestration, Gateway publication, and `CELL0.5` availability remain |
| Applications, Knowledge, Automations, Inference | Ownership and staged architecture are frozen | `APP0`, `K0`, `AUT0`, `PW0`, and `I0` remain unavailable |

Notifications now include exact-recipient in-app projections, immutable personal
outbound subscriptions authored as canonical A3S ACL, and transactional
`notification.delivery.requested` facts. Side-effect-free signed-webhook and
Slack-compatible builders feed a NATS-only durable/manual-ack A3S Event consumer
through the fenced Connector application service. The consumer validates the
persisted delivery authorization and commits one monotonic Delivered, Rejected,
Indeterminate, or Exhausted receipt before ACK. Redelivery replays durable C6
evidence, and receipt-commit/ACK loss becomes ACK-only without another Provider
call. Retryable C6 evidence defers later generations until its exact
`Retry-After` deadline, then a fixed eight-attempt budget terminates from the
eighth immutable evidence record without a ninth Provider call. A3S Event remains
the only waiting/redelivery mechanism; no Notification retry table, mutable
counter, token bucket, timer, queue, or scheduler is introduced. User-configured
alert suppression/delivery budgets, SMTP, Workflow Flow composition, the intentionally
deferred Web surface, and production availability remain gated. A retained
PostgreSQL 17 plus real NATS gate verifies terminal C6 evidence before ACK and
ACK-only replay after durable-consumer restart.
REST/OpenAPI, the maintained client, CLI, and four Management MCP tools expose
recipient-bound create/list/get/revoke through the same Notifications CQRS,
Resource Grant, idempotency, Outbox, audit, and PostgreSQL repository authority.

### Latest Workflow contract slice

Migration `108` adds no table or execution mechanism. It permits one optional
immutable `cloud.workflow.composite-regions.v1` child beside the three
mandatory WorkflowRevision semantic contracts and optional variable defaults.
The contract freezes:

- Iteration item/concurrency bounds and `terminate`, `continue_null`, or
  `remove_failed` failure behavior.
- Loop iteration/time bounds and an exact termination-result path.
- Exact coverage of every `composite_region` descriptor using
  `workflow.iteration` or `workflow.loop` semantics.
- One non-nil child Workflow revision capability for each region.
- Exact composite-policy digest pins in Plan v2 and immutable WorkflowRun v2
  input.

Legacy revisions and Plan/Run bytes retain their prior shape when the optional
material is absent. New composite publications fail closed without it. Runtime
dispatch still rejects `subworkflow`; a later slice must execute these exact
contracts through existing A3S Flow primitives rather than introducing a
Cloud scheduler or region state store.

### Capabilities remain first-class

Website simplification and staged delivery never delete product outcomes or
transfer their owners.

| Capability group | Preserved outcome |
| --- | --- |
| Governance | Organizations, projects, environments, identity, grants, REST, CLI, Web, Search, Management MCP, audit, and notifications |
| Source and artifacts | External Git, webhooks, immutable revisions, reproducible Box builds, provenance, previews, monorepos, imports, and hosted releases |
| Compute and fleet | Finite Tasks, Services, cancellation, cleanup, placement, rollout, Claims, outbound mTLS, commands, receipts, fencing, draining, and recovery |
| Traffic and data | Domains, TLS, Gateway scopes, routes, Secrets, immutable objects, volumes, databases, backup, restore, retention, writer fencing, and named Durable Cells |
| Agents and Workflow | Conversations, approvals, checkpoints, Tools, Skills, MCP, models, typed Workflows, HumanTasks, providers, compensation, evaluation, promotion, and rollback |
| Application platform | Six application projections, sessions/messages, conversation variables, files, RAG Knowledge, Knowledge Pipelines, triggers, connectors, publication, monitoring, feedback, and enterprise policy |
| Inference | Power-hosted models, accelerator Claims, provider policy, scoped keys, routing/fallback, usage, and governed self-service |

The exact ownership, dependency, and public-parity gates are normative in the
[AI application platform plan](docs/ai-application-platform-plan.md) and
[ROADMAP.md](ROADMAP.md).

## Quick start

The shortest path starts the API directly; no frontend process is required.

### Requirements

- Rust 1.88 or later
- PostgreSQL 17 or a compatible supported release
- A3S Box for node-local workload and build execution
- The pinned A3S Gateway revision for routed services
- Bun only for the TypeScript client or CLI
- NATS JetStream only when the NATS A3S Event provider is selected

Redis is not required and is never durable business, workflow, queue, session,
lock, or replay authority.

### Run the control plane

```bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

Migrations run during startup. The development profile listens on
`127.0.0.1:8080` and uses the in-memory A3S Event provider.

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
```

The committed [`openapi/v1.json`](openapi/v1.json) snapshot is REST major
version 1, contract `1.37.0`.

### Bootstrap the first organization

Cloud stores only the API-token digest. The caller creates and retains the
first credential.

```bash
export A3S_CLOUD_ADMIN_TOKEN="a3s_$(openssl rand -hex 32)"

curl --request POST http://127.0.0.1:8080/api/v1/bootstrap \
  --header "content-type: application/json" \
  --header "idempotency-key: local-bootstrap" \
  --header "x-a3s-bootstrap-token: ${A3S_CLOUD_BOOTSTRAP_TOKEN}" \
  --data "{\"organizationName\":\"Local\",\"tokenName\":\"local-admin\",\"token\":\"${A3S_CLOUD_ADMIN_TOKEN}\",\"expiresAt\":null}"
```

Subsequent requests use `Authorization: Bearer ${A3S_CLOUD_ADMIN_TOKEN}`.
Every mutation requires a stable `idempotency-key`.

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
written to a CLI context file. See the [CLI reference](cli/README.md).

## Management surfaces

| Surface | Contract |
| --- | --- |
| REST | Versioned `/api/v1`, common envelopes, request IDs, idempotency, and committed OpenAPI |
| TypeScript client | Maintained adapter in [`packages/cloud-client`](packages/cloud-client) |
| CLI | Automation surface in [`cli`](cli) with JSON output and no token argument |
| Management MCP | Sessionless, tenant-authorized tools documented in [Management MCP](docs/management-mcp.md) |
| Web | Retained authenticated projection over the same client and application layer |

Controllers and adapters stay thin: they do not call providers directly,
invent presentation-owned state, or create interface-specific lifecycles.

## Configuration

Cloud and the Node Agent use closed, validated A3S ACL. Unknown fields and
unsafe timing relationships fail before the process starts. Secret values do
not belong in ACL.

| Area | Responsibility |
| --- | --- |
| `server`, `auth`, `postgres` | Roles, bootstrap, identity, and durable state |
| `events`, `operations` | Outbox publication and durable operation timing |
| `node_control`, `fleet` | Outbound mTLS, leases, inventories, observations, and Claims |
| `deployments`, `executions`, `builds`, `artifacts` | Workload, Task, Box build, and immutable-content bounds |
| `registry`, `sources` | OCI publication and external Git policy |
| `edge`, `gateway` | Routes, certificates, snapshot validity, and native Gateway apply |
| `logs`, `security`, `box` | Durable logs, production trust, isolation, and transient Secret materialization |

Use [`config/cloud.acl`](config/cloud.acl) and
[`config/node.example.acl`](config/node.example.acl) as executable references.

## Repository

```text
Cloud/
|-- crates/
|   |-- contracts/       # versioned cross-process contracts
|   |-- control-plane/   # API, domain modules, workers, and persistence
|   |-- node-agent/      # outbound node protocol and execution adapters
|   `-- web-server/      # bounded private static-content server
|-- migrations/          # PostgreSQL schema evolution
|-- config/              # closed A3S ACL configuration
|-- openapi/             # committed REST contract
|-- packages/cloud-client/
|-- cli/
|-- tools/               # provider and recovery gates
|-- docs/                # architecture, plans, decisions, and runbooks
|-- web/                 # retained authenticated operations console
|-- website/             # retained public site and versioned docs
`-- architecture-3d/     # interactive architecture projection
```

This directory is its own Rust workspace inside the wider A3S monorepo.

## Development and verification

Run Rust validation from the Cloud repository root:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Real-provider and release certification runs on an isolated Linux host. Use
the repository-owned gates:

- [`C0.1` cross-surface conformance](tools/c0-conformance/README.md)
- [Runtime conformance](tools/runtime-conformance/README.md)
- [A3S Box provider conformance](tools/box-conformance/README.md)
- [Pinned Gateway conformance revision](tools/gateway-conformance/gateway-revision)

Client, CLI, Web, website, contract, and compatibility checks remain in CI.

## Documentation

| Document | Owns |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, and execution order |
| [Technical architecture](docs/architecture.md) | Ownership, topology, consistency, and failure behavior |
| [Development plan](docs/development-plan.md) | Implementation slices and exit evidence |
| [Domain model](docs/domain-model.md) | Aggregates, state machines, and invariants |
| [Workflow and evolution](docs/workflow-evolution-plan.md) | `W0`, heterogeneous `A1`, and governed `EV0` contracts |
| [AI application platform](docs/ai-application-platform-plan.md) | `APP0`, `K0`, `AUT0`, node coverage, Flow-preservation rules, and parity evidence |
| [Durable Cell Service](docs/durable-cell-platform-plan.md) | `CELL0` ownership, fencing, provider boundary, ordered gates, and fault evidence |
| [Architecture decisions](docs/decisions/app-platform/README.md) | Normative authority boundaries for application-platform work |
| [Inference plan](docs/inference-plan.md) | Model, provider, routing, usage, and conformance design |
| [Management MCP](docs/management-mcp.md) | Protocol, authorization, and tool contract |

## License

[MIT](LICENSE) &copy; 2026 A3S Lab
