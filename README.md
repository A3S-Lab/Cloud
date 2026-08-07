# A3S Cloud

<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Cloud: intent in, exact state out" />
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Cloud/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <img alt="Rust 1.88 or later" src="https://img.shields.io/badge/Rust-1.88%2B-1f2a23?logo=rust&amp;logoColor=white" />
  <a href="openapi/v1.json"><img alt="REST contract 1.8.0" src="https://img.shields.io/badge/REST_contract-1.8.0-2872b8" /></a>
  <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-b8f36b?labelColor=1f2a23" /></a>
</p>

<p align="center">
  <a href="#product-system">Products</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#backend-quick-start">Quick start</a> &middot;
  <a href="#capabilities-that-remain-first-class">Capabilities</a> &middot;
  <a href="#delivery-status">Status</a> &middot;
  <a href="#documentation">Documentation</a>
</p>

**A3S Cloud is a self-hosted control plane for operating applications, Agents,
MCP servers, Workflows, and model services on infrastructure you own.** It
turns authorized tenant intent into versioned, exact applied state through one
durable control loop, one outbound node channel, and one provider-neutral
execution path.

> [!IMPORTANT]
> The architecture describes the stable target, not blanket availability.
> Public availability is decided by the exact gates in
> [ROADMAP.md](ROADMAP.md). The active delivery policy is backend-first: domain
> contracts, persistence, providers, REST/OpenAPI, the maintained client, CLI,
> Management MCP, and real recovery evidence come before new frontend work.

## Product system

Three outward-facing products compose the same Cloud authorities. Their order
matches the public product story; none creates a separate control plane,
scheduler, runtime, queue, or evidence store.

| Product | Outcome | Shared foundation |
| --- | --- | --- |
| **01 / Unified Gateway** | Give Workflow, Agent, MCP, model, and application traffic one governed cloud-edge entry with identity, protocol policy, routing, health, and evidence | Cloud API and Identity own management policy; Edge owns desired traffic; A3S Gateway alone owns the applied byte path |
| **02 / Workflow Orchestration** | Compile ontology-defined objects, relationships, rules, goals, and constraints into typed, recoverable execution | Workflow owns semantics while Cloud Operations and A3S Flow remain the sole durable orchestration path |
| **03 / Agent Factory** | Turn heterogeneous Harness implementations into immutable, evaluated, deployable Agent products | Assets, Agents, Workloads, Fleet, Runtime, Box, and one provider-neutral `AgentExecutionProvider` contract |

Security operations remain inside Unified Gateway as tenant-scoped correlation
over Gateway, Runtime, Box, Agent, A3S Sentry, AnySentry, and audit evidence.
A3S Code is one first-party Harness provider, not a privileged parallel
execution architecture.

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="A3S Cloud products converging through one control, execution, and request architecture" />
</p>

The current architecture separates three paths that must never be collapsed:

1. **Control path:** accepted mutations atomically commit desired state, replay
   identity, an Operation, and bounded Outbox facts to PostgreSQL.
2. **Execution path:** Workloads and Fleet reserve exact Claims and send one
   versioned command through the outbound-only Node Agent; Runtime exposes Task
   and Service lifecycle, and Box is the sole local execution/build provider.
3. **Request path:** A3S Gateway sends bytes directly to an exact healthy
   application, Harness, MCP, or Power endpoint. Cloud stays off this path and
   advances only from the matching applied acknowledgement.

The control plane is a modular monolith. Its `api`, `worker`, and `relay` roles
can run together or separately from the same binary. PostgreSQL remains the
business authority in every profile; A3S Event accelerates committed facts but
does not replace recovery scans.

Explore the [interactive architecture](https://a3s-lab.github.io/Cloud/architecture/)
or read the [technical architecture](docs/architecture.md) for bounded
contexts, dependency directions, consistency boundaries, protocols, failure
behavior, and the complete capability-preservation register.

### One Gateway publication path

<p align="center">
  <img src="assets/readme/gateway-publication.svg" width="100%" alt="Ordinary Routes and hosted MCP share one node desired-state planner, complete Gateway snapshot compiler, atomic publication owner, Fleet command, and exact acknowledgement path" />
</p>

Ordinary Route changes and hosted MCP reconciliation never publish independent
snapshots. Both enter one node-scoped desired-state planner, which rereads the
complete ordinary Route set and every active or previously published logical
MCP scope. One compiler then produces the complete routes, targets,
certificates, expiry, and canonical digest for that physical Gateway.

Atomic staging records the exact scope-set CAS, snapshot revision, command, and
one durable publication owner. The originating ordinary flow dispatches an
ordinary-owned marker; the MCP reconciler dispatches only an MCP-owned marker.
Both reuse the same Fleet command and acknowledgement projection, so one path
cannot erase the other's routes or dispatch the same snapshot twice. Cloud
advances installed state only from the matching Gateway acknowledgement.

This is the current Cloud `MCP0.3` foundation, not a hosted-MCP availability
claim. Real Runtime, Box, Gateway, process-loss, recovery, and cleanup evidence
must still close the joint gates in [ROADMAP.md](ROADMAP.md).

### One concern, one authority

| Concern | Sole authority | Duplicate mechanism that is prohibited |
| --- | --- | --- |
| Business desired state | PostgreSQL through A3S ORM | Redis, an event stream, a node journal, or a local file as product truth |
| Long-running work | A3S Flow plus Cloud Operations | Product-specific workflow engines, queues, or retry loops |
| Placement, replicas, rollout, scaling | Workloads | Agent-, MCP-, inference-, Gateway-, or import-specific schedulers |
| Node delivery and hard-resource ownership | Fleet commands, Node Agent journal, and Fleet Claims | A second node channel, direct process control, or in-memory reservations |
| Provider-neutral lifecycle | A3S Runtime Task and Service | Product policy inside Runtime or direct provider calls from a Cloud context |
| Local execution and builds | A3S Box | Docker, BuildKit, another Runtime driver, or a Cloud executor |
| Routing intent, snapshot publication, and applied traffic | Edge owns one node planner/compiler and durable publication owner; Fleet delivers one command; A3S Gateway owns applied state | Ordinary- or MCP-specific publishers, Cloud proxying, Gateway-owned tenant policy, or inferred apply success |
| Plugin assignments and package lifecycle | Cloud Plugins for tenant intent; shared A3S Use Plugin Manager for package generations | A Cloud installer, catalog copy, grant store, binding store, or generic plugin RPC |
| Immutable bytes | One shared content-addressed object client with typed domain adapters | Parallel filesystem/S3 clients or untyped cross-domain blob APIs |
| Management behavior | One application command/query layer | REST-, CLI-, MCP-, or Web-specific business state and rules |

## Backend quick start

The shortest path starts the API directly; no frontend process is required.

### Requirements

- Rust 1.88 or later
- PostgreSQL 17 or a compatible supported release
- A3S Box for node-local workload and build execution
- The pinned A3S Gateway revision when exercising routed services
- A3S Power only when exercising the future inference profile
- Bun only for the TypeScript client or CLI
- NATS JetStream only when the NATS A3S Event provider is selected

Redis is not required by the current profile and is never durable business,
workflow, queue, session, lock, or replay authority.

### Run the control plane

```bash
export A3S_CLOUD_POSTGRES_URL="postgres://a3s_cloud:replace-me@127.0.0.1:5432/a3s_cloud"
export A3S_CLOUD_BOOTSTRAP_TOKEN="replace-with-at-least-32-random-characters"
export A3S_CLOUD_GITHUB_WEBHOOK_SECRET="replace-with-32-to-512-random-bytes"

cargo run -p a3s-cloud-control-plane -- config/cloud.acl
```

Migrations run during startup. The default development profile listens on
`127.0.0.1:8080` and uses the in-memory A3S Event provider.

```bash
curl http://127.0.0.1:8080/api/v1/health/live
curl http://127.0.0.1:8080/api/v1/health/ready
curl http://127.0.0.1:8080/api/v1/openapi.json
```

The raw OpenAPI document is the committed
[`openapi/v1.json`](openapi/v1.json) snapshot for REST major version 1 and
contract version `1.8.0`.

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

The CLI and maintained TypeScript client use the same REST contract and
application behavior. Credentials are accepted from environment variables or
standard input where a command creates sensitive material; they are never
written to a CLI context file.

```bash
bun install --cwd cli --frozen-lockfile

export A3S_CLOUD_TOKEN="${A3S_CLOUD_ADMIN_TOKEN}"
export A3S_CLOUD_URL="http://127.0.0.1:8080/api/v1"

bun run --cwd cli src/main.ts diagnostics status --output=json
bun run --cwd cli src/main.ts organizations list --output=json
bun run --cwd cli src/main.ts operations list --output=json
```

See the [CLI reference](cli/README.md) for contexts, resource commands,
standard-input credential handling, output contracts, and exit codes.

## Capabilities that remain first-class

The public website is intentionally simpler than the product architecture.
Omission from a website diagram never retires a Cloud capability or transfers
its authority.

| Capability group | Preserved Cloud outcome | Authority and gates |
| --- | --- | --- |
| Governance and management | Organizations, projects, environments, identity, memberships, grants, REST, CLI, Web, Search, and Management MCP | Identity, Projects, `F0`, `C0` |
| Source delivery | External Git sources, webhooks, immutable revisions, reproducible Box builds, provenance, previews, monorepos, and imports | Sources, Artifacts, `G0`, `P0` |
| Hosted assets and plugins | Hosted Git, immutable Agent/MCP/Skill releases, Skill binding, and exact A3S Use assignments | Assets, Plugins, `A0`, `U0` |
| Generic compute | Finite Tasks, ordinary application Services, cancellation, cleanup, and operation replay | Executions, Workloads, `R0`, `D0`, `BX0` |
| Fleet control | Enrollment, outbound mTLS, inventory, Claims, commands, receipts, fencing, draining, and cleanup | Fleet, Node Agent, `N0`, `H0` |
| Execution substrate | Runtime/Box isolation, images, builds, mounts, outputs, logs, checkpoints, health, and provider recovery | Runtime, Box, `BX0`, `PW0` |
| Managed traffic | Domains, certificate issuance and ownership-exclusive renewal, logical Gateway scopes, one node planner/compiler, owner-exclusive complete publication, routing, health, update, rollback, and exact applied state | Edge, Fleet, Gateway, Secrets, `E0`, `H0`, `MCP0`, `I0` |
| Data and trust | Secret versions, immutable objects, persistent volumes, databases, backup, restore, retention, and writer fencing | Secrets, Artifacts, Data, `S0` |
| Operations and evidence | Idempotency, Operations, Flow, Outbox/Event, audit, notifications, logs, metrics, traces, Search, and runbooks | Shared mechanisms, `F0`, `C0`, `H0` |
| Agentic execution | Conversations, semantic events, approvals, suspension, checkpoints, forks, trajectories, Tools, Skills, MCP, models, and provider-neutral Harnesses | Agents over the common path, `A0`, `A1`, `MCP0`, `I0` |
| Workflow and evolution | Ontologies, immutable plans, human decisions, governed evidence datasets, evaluation, promotion, canary halt, and exact rollback | Workflow and Evolution semantics over Flow/Operations, `W0`, `EV0` |
| Inference | Power-hosted model Services, accelerator Claims, model/provider policy, scoped keys, routing/fallback, durable usage, and governed self-service | Inference, Power, Workloads, Fleet, Edge, Gateway, `PW0`, `I0` |

### TokenHub and Google AX outcomes remain explicit

A3S Cloud keeps the useful outcomes while refusing the duplicate control
mechanisms of the reference products.

| Reference outcome | A3S-owned design | Availability boundary | Not copied |
| --- | --- | --- | --- |
| TokenHub-style private multi-provider model gateway, model catalog, priority/weight routing, fallback, and health diagnostics | Inference owns immutable model/provider/policy revisions; Edge owns route intent; Gateway applies the typed data-plane snapshot | Planned `I0.2b`, `I0.2d`, `I0.5`, and optional `I0.6` | TokenHub API/storage topology, provider-native desired state, a second proxy, or Gateway-owned management state |
| TokenHub-style workspaces, enterprise sign-in, RBAC, scoped keys, quotas, and concurrency policy | Identity owns principals, memberships, grants, credentials, and revocation; `C0` owns authorized surfaces; Inference owns model access policy | `C0.1`/`C0.2` foundations are verified; external OIDC and role-focused policy are planned in `C0.3`; model/key self-service is planned in `I0.2e` | A second identity/key store, browser-only authorization, or plaintext credential recovery |
| TokenHub-style usage, request attribution, diagnostics, API exploration, and cost showback | Gateway emits bounded request/attempt facts; Inference owns the durable usage ledger; `C0` owns authorized project views | Planned `I0.2c`, `C0.3`, and `I0.2e` | Prompts/responses in management telemetry, client-side usage truth, or commercial billing authority |
| TokenHub-style protocol and provider breadth | Separately versioned `InferenceProtocolProfile` contracts and credential-isolated providers behind the same Inference, Edge, Gateway, Secret, and usage boundaries | Optional post-production `I0.6`, only after real protocol, terms, credential, usage, failure, and recovery conformance | An untyped byte proxy, browser-held upstream credentials, or implied support for every vendor |
| Google AX-style isolated distributed Harness execution and bring-your-own Harness | One Agents-owned `AgentExecutionProvider`; Workloads, Fleet, Runtime, and Box own placement, delivery, isolation, and lifecycle | `A1.0` verified; `A1.1` implemented; native Code `A1.2` awaits verification; `A1.3` onward is gate-driven | AX server/controller deployment, a provider scheduler, a separate run store, or direct Harness clients |
| Google AX-style replay, approvals, pause/resume, checkpoints, forks, trajectories, and telemetry | One PostgreSQL Agent semantic sequence, shared cursor/SSE transport, immutable checkpoints, and the common Operation/Fleet/provider recovery path | Foundations exist; complete governance and recovery remain planned in `A1.5` and `A1.6` | AX event-log authority, Flow history as transcript, Runtime logs as semantic state, or a second checkpoint store |
| Google AX-style per-execution Harness, instruction, environment, model, Skill, MCP, and Tool customization | One immutable, closed `HarnessInvocationProfile` binds exact release and Secret references before dispatch | Planned `A1.4`, after the provider-neutral `A1.3` contract and applicable `A0`/`MCP0`/`I0` identities | Mutable provider JSON as desired state, arbitrary environment injection, copied Secret material, or provider-owned authorization |

These rows preserve product intent; they do not claim that planned gates are
already available. Removing a reference name from a site or navigation label
does not authorize deleting the A3S-owned outcome.

## Delivery status

Status is gate-driven rather than date-driven. **Verified** means the complete
real-provider, failure, recovery, cleanup, and release evidence passes.
**Historical** means prior regression evidence exists but does not certify the
current Box-only provider contract.

| Gate | Product outcome | State |
| --- | --- | --- |
| `BX0` | Sole A3S Box execution/build path and re-certification of the complete baseline | In progress |
| `PW0` | Immutable ACL-native Power Service profile and inference boundary | Planned |
| `R0` | Universal Runtime Task and Service contract | Historical; Box re-certification pending |
| `F0` | Boot API, PostgreSQL, tenancy, identity, Flow, Outbox, and projections | **Verified** |
| `N0` | Enrollment, outbound mTLS, commands, observations, journal, and sole Box driver | Historical; Box re-certification pending |
| `D0` | Digest-pinned Workloads, scheduling, activation, cancellation, and recovery | Historical; Box re-certification pending |
| `E0` | TLS, Gateway snapshots, Secrets, logs, update, rollback, and clean-host recovery | Historical; Box re-certification pending |
| `G0` | External source resolution, Box builds, OCI publication, provenance, and deployment handoff | In progress |
| `P0` | Build detection, workload profiles, previews, monorepos, and closed Compose import | Planned |
| `C0` | REST/CLI/Management MCP parity, OIDC, grants, collaboration, investigation, notifications, audit, and bounded exec | In progress |
| `A0` | Immutable Agent/MCP/Skill release catalog, Agent deployment, and Skill binding | In progress |
| `U0` | Exact A3S Use registry and workspace package assignments through the shared Plugin Manager | In progress; unavailable |
| `MCP0` | Modern hosted MCP admission, Runtime hosting, orchestration, Gateway enforcement, and recovery | Cloud orchestration foundation in progress; unavailable until the joint release gate |
| `A1` | Heterogeneous Agent execution, semantic events, approvals, checkpoints, forks, and trajectories | In progress (`A1.0` verified; `A1.1` implemented; native Code `A1.2` pending verification) |
| `W0` | Ontology-driven Workflow planning and recoverable typed execution | Planned |
| `S0` | Stateful databases, objects, volumes, fencing, backup, restore, and retention | Planned |
| `H0` | Replicas, multi-node placement, networking, Gateway replication, HA, and autoscaling | In progress |
| `I0` | Accelerator-backed model serving, providers, routing, keys, usage, and self-service | Planned |
| `EV0` | Governed evidence, evaluation, Agentic RL candidates, promotion, canary halt, and rollback | Planned |

The full evidence, dependencies, and backend execution order live in the
[product roadmap](ROADMAP.md). Current priorities close the Box-only baseline
and external-source evidence, bind hosted MCP to immutable releases, complete
the Runtime/Cloud/Gateway MCP contract, establish Power as the first inference
backend, and advance identity, plugin, stateful, Agent, Workflow, and scale
contracts only through their existing authorities.

## Management interfaces

| Interface | Contract |
| --- | --- |
| REST | Versioned `/api/v1` API with a common success/error envelope, request ID, idempotency, and committed OpenAPI snapshot |
| TypeScript client | Maintained typed adapter in [`packages/cloud-client`](packages/cloud-client) |
| CLI | Automation adapter in [`cli`](cli) with JSON output and no token argument |
| Management MCP | Sessionless, tenant-authorized management tools documented in [management MCP](docs/management-mcp.md) |
| Web | Retained authenticated projection over the same client and application layer; new frontend feature work is currently frozen |

Controllers and adapters remain thin. They cannot call providers directly,
invent presentation-owned state, or create interface-specific lifecycle
mechanisms.

## Tenancy and boundaries

```text
Organization
└── Project
    └── Environment
        ├── sources and BuildRuns
        ├── desired workload revisions
        ├── deployments and Operations
        └── routes, Secrets, and observations
```

Authentication is global except for bootstrap and health endpoints. Commands
and queries enforce organization ownership and resource scope at the
application boundary.

## Configuration

Cloud and the Node Agent use closed, validated A3S ACL. Unknown fields and
unsafe timing relationships fail before the relevant process starts. Secret
values do not belong in ACL.

| Area | Responsibility |
| --- | --- |
| `server`, `auth`, `postgres` | API roles, bootstrap, identity, and durable state |
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
|-- tools/               # real-provider and recovery gates
|-- docs/                # architecture, domain, plans, and runbooks
|-- web/                 # retained authenticated operations console
|-- website/             # retained public site and versioned docs
`-- architecture-3d/     # retained interactive architecture projection
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
the repository-owned gates rather than copying partial commands:

- [`C0.1` cross-surface conformance](tools/c0-conformance/README.md)
- [Runtime conformance](tools/runtime-conformance/README.md)
- [A3S Box provider conformance](tools/box-conformance/README.md)
- [Pinned Gateway conformance revision](tools/gateway-conformance/gateway-revision)

Existing client, CLI, Web, and website checks remain in CI. The backend-first
freeze defers new visual work; it does not remove those retained projections or
their eventual gate obligations.

## Documentation

| Document | Owns |
| --- | --- |
| [Product roadmap](ROADMAP.md) | Gate status, dependencies, and execution order |
| [Technical architecture](docs/architecture.md) | Stable ownership, topology, consistency, and failure behavior |
| [Development plan](docs/development-plan.md) | Implementation slices and exit evidence |
| [Domain model](docs/domain-model.md) | Aggregates, state machines, and invariants |
| [Workflow and evolution plan](docs/workflow-evolution-plan.md) | `W0`, heterogeneous `A1`, and governed `EV0` contracts |
| [Inference plan](docs/inference-plan.md) | `I0` model, provider, routing, usage, and conformance design |
| [Ephemeral executions](docs/executions.md) | Finite Task API and cleanup lifecycle |
| [Management MCP](docs/management-mcp.md) | Protocol, authorization, and tool contract |

## License

[MIT](LICENSE) &copy; 2026 A3S Lab
