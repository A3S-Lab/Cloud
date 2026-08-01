<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Workflow — design durable AI workflows and execute every node through A3S Runtime">
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Workflow/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Workflow/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="Rust 1.88+" src="https://img.shields.io/badge/Rust-1.88%2B-101118?logo=rust&logoColor=white">
  <img alt="Bun 1.3.14" src="https://img.shields.io/badge/Bun-1.3.14-101118?logo=bun&logoColor=white">
  <img alt="PostgreSQL" src="https://img.shields.io/badge/PostgreSQL-source%20of%20truth-2587f5?logo=postgresql&logoColor=white">
  <img alt="A3S Runtime" src="https://img.shields.io/badge/A3S%20Runtime-every%20node-7137d8">
</p>

<p align="center">
  <strong>Design durable AI workflows. Run every node in the right Runtime.</strong><br>
  A PostgreSQL-native engine for Studio, coding agents, standalone deployments, and A3S Cloud.
</p>

<p align="center">
  <a href="#see-it-work">Studio</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#architecture-today">Architecture</a> ·
  <a href="#standalone-or-embedded-in-a3s-cloud">Cloud embedding</a> ·
  <a href="#composable-designer-and-agentic-node-direction">Design direction</a> ·
  <a href="#runtime-contract">Runtime contract</a>
</p>

## See it work

<p align="center">
  <img src="assets/readme/studio.png" width="100%" alt="The Chinese A3S Workflow Studio showing a curved input, template, and output graph with A3S Runtime connected">
</p>

The Studio is a real client of the same API used by the CLI and
`$a3s-workflow` Skill. It edits an optimistic workflow version, keeps curved
connections as the default, starts a durable run, and projects Runtime state
back onto the graph.

### Available now

| Surface | Current capability |
| --- | --- |
| Studio | Chinese graph editor, node library, inspector, Runtime policy, run panel, tracing, and evidence |
| Control plane | Durable definitions, runs, queues, hooks, memory, and execution evidence in PostgreSQL |
| Execution | Every built-in node becomes a digest-bound A3S Runtime unit |
| Coding agents | Machine-readable Rust CLI and the `$a3s-workflow` Skill |
| Delivery | Cross-platform installers and a Docker Compose development stack |
| Acceptance | Local browser-to-Runtime scenarios through the official A3S Test CLI |

## Quick start

One command installs the CLI and Skill, deploys the full stack, waits for
health, and opens the Studio.

macOS / Linux:

```bash
./scripts/install.sh
```

Windows PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
```

Prerequisites are Docker Desktop or Docker Engine with Compose v2, plus Rust
1.88+ when installing the CLI. Use `--no-cli`, `--no-skill`, or
`--no-deploy` on macOS/Linux and `-NoCli`, `-NoSkill`, or `-NoDeploy`
on Windows to install only the required surfaces.

The Studio opens at <http://localhost:3000>. Verify the machine-readable API:

```bash
a3s-workflow health
a3s-workflow node-types
a3s-workflow workflow list
```

A coding agent can start with:

```text
Use $a3s-workflow to author, run, and inspect a Runtime-backed coding workflow.
```

## Why A3S Workflow

AI workflows are not ordinary background jobs. Model calls are expensive,
Agents may pause for tools or people, and one graph may need CPU, GPU, sandbox,
or confidential pools.

| Product invariant | Consequence |
| --- | --- |
| **Every node is Runtime-native** | API and Flow workers never execute node business logic in-process; start and output cross the same boundary |
| **PostgreSQL is authoritative** | Definitions, event history, leases, hooks, memory, and evidence survive process failure in one durable store |
| **Placement is part of the graph** | Provider, pool, resources, isolation, network, and secret references are explicit per node |
| **Stateless capacity scales independently** | API replicas, Flow workers, Runtime providers, and provider pools scale on different axes |
| **One core supports two hosts** | Standalone services and the planned A3S Cloud module share graph and run semantics instead of synchronizing two engines |
| **AI changes remain reviewable** | The target Designer architecture applies structured, revision-bound patches instead of hidden graph mutation |
| **Execution is inspectable** | Unit ID, generation, spec digest, observation, output digest, and usage remain attached to each attempt |

### Why PostgreSQL instead of Redis?

The control plane needs transactions, ordered event streams, optimistic graph
versions, recoverable leases, queryable audit records, and durable interaction
hooks. PostgreSQL already provides that recovery boundary. Redis may become an
optional cache, but it will not own workflow truth.

## Architecture today

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="Studio and coding-agent clients call A3S Boot and A3S Flow backed by PostgreSQL, while every ready node is dispatched to independently scalable A3S Runtime pools">
</p>

The API validates and versions a desired graph. A3S Flow appends run events and
leases ready work through PostgreSQL. Each ready node is converted into an
immutable Runtime specification; the selected provider returns an observed,
digest-verified result to durable history.

- API replicas serve graph, run, memory, approval, and evidence endpoints.
- Flow workers claim queue rows with `SKIP LOCKED` and recover expired leases.
- Runtime providers own lifecycle, isolation, resources, secrets, networking,
  artifacts, and logs.
- PostgreSQL is the only required infrastructure dependency.

Read the detailed [architecture](docs/architecture.md) and
[Runtime provider contract](docs/runtime-providers.md).

## Standalone or embedded in A3S Cloud

A3S Workflow is being decomposed into a reusable engine core with two
composition roots. The existing standalone host owns its HTTP surface,
PostgreSQL adapter, Flow worker, and Runtime router. The target
[A3S Cloud](https://github.com/A3S-Lab/Cloud) host imports the same engine as an
A3S Boot module and injects platform services, so embedding does not require a
second network hop.

This is an **architecture target**. The standalone path is available today;
the reusable crates and Cloud composition adapter are the next extraction
boundary.

| Concern | Workflow owns | A3S Cloud injects in embedded mode |
| --- | --- | --- |
| Domain | Graph validation, publication, run state machine, node attempts, hooks, and output semantics | Organization, project, environment, principal, and policy context |
| Commands and queries | Host-neutral application handlers and stable result types | A3S Boot `CommandBus`, `QueryBus`, guards, routes, and management surfaces |
| Durability | Repository, transaction, clock, event, migration, and Flow ports | Cloud PostgreSQL, migration runner, transactional outbox, and A3S Flow operation host |
| Node execution | Immutable invocation, placement intent, generation fencing, and result verification | Cloud Execution/Fleet dispatch, node agents, A3S Runtime lifecycle, secrets, artifacts, and logs |
| Web | Controlled Designer SDK and typed host adapters | A3S Cloud shell, identity, navigation, project context, and API client |
| Configuration | Typed engine settings | Values decoded from Cloud A3S ACL; no parallel product configuration file |

The target package boundary is deliberately inward-facing:

```text
workflow-domain        graph + run invariants; no HTTP, SQL, Boot, or Runtime
workflow-application   commands, queries, use cases, and outbound ports
workflow-engine        reducer, scheduling decisions, hooks, and Runtime protocol
workflow-protocol      versioned node invocation and result wire contract (available)
workflow-adapters      PostgreSQL, A3S Flow, A3S Runtime, events, and artifacts
workflow-boot-module   controllers, guards, CQRS registration, and health hooks
workflow-host          standalone composition root and cross-platform delivery
```

In Cloud, every workflow, version, run, node attempt, and interaction is scoped
by `organization_id`, `project_id`, and `environment_id`. A Workflow Run ID is
correlated with Cloud Operation and Execution IDs, while idempotency keys,
aggregate versions, Runtime generations, and content digests fence replay.
Cloud remains the sole owner of identity, tenancy, infrastructure placement,
capacity, secrets, artifacts, and Runtime lifecycle—there is no second
scheduler or provider registry inside the embedded engine.

The intended composition API is shown below for direction only:

```rust
let workflow = WorkflowModule::builder()
    .tenant_context(cloud_tenant_context)
    .repositories(cloud_postgres)
    .flow(cloud_flow)
    .runtime(cloud_execution_dispatch)
    .events(cloud_outbox)
    .build()?;

cloud.import(workflow);
```

## Composable Designer and Agentic node direction

<p align="center">
  <img src="assets/readme/designer-architecture.svg" width="100%" alt="Target architecture where standalone A3S Workflow and A3S Cloud embed one workflow core, share injected Flow, PostgreSQL, and Runtime services, and compose A3S Form and A3S Code nodes">
</p>

The diagram above combines the **target platform architecture**. The durable
standalone control and execution planes are shipped; the reusable engine,
Cloud module, Designer, descriptor-driven node UI, A3S Form integration, and
dedicated A3S Code executor family are the next contract boundaries.

| Boundary | Responsibility | Status |
| --- | --- | --- |
| Embeddable Workflow core | Host-neutral domain/application crates with injected persistence, Flow, Runtime, event, artifact, and tenant ports | Architecture target |
| A3S Cloud module | A3S Boot/CQRS composition adapter using Cloud tenancy, PostgreSQL, operations, Execution/Fleet, secrets, and outbox | Architecture target |
| Workflow Designer SDK | Controlled graph editing, selection, diagnostics, patch review, and run projections | Architecture target |
| Node Descriptor | Versioned presentation, ports, configuration form reference, Runtime requirements, and executor family | Architecture target |
| [A3S Form](https://github.com/A3S-Lab/Form) | Independent AI Native Form Designer and schema-driven node configuration renderer | Architecture incubation |
| Standard node runner | Start, template, LLM, bounded Agent, tool, router, memory, HTTP, approval, and output | Available |
| [A3S Code](https://github.com/A3S-Lab/Code) Agentic runner | Governed sessions, workspaces, Skills, MCP, tools, planning, checkpoints, and replay | Architecture target |
| Durable interaction bridge | Flow hooks for approval and resumable Agent tool confirmation | Architecture target |

The intended contract is compact:

```text
NodeDescriptor
├── presentation       icon, renderer, safe summary fields
├── ports              typed static or declarative dynamic ports
├── configuration      immutable a3s-form reference + defaults
├── executor           standard | a3s-code | custom
└── runtime            capabilities + placement constraints
```

The Workflow repository remains the owner of graph semantics, publishing,
orchestration decisions, and Runtime invocation contracts. A3S Cloud owns the
platform control plane, A3S Form owns form compilation and rendering, and A3S
Code owns the governed Agent session. None becomes a second workflow source of
truth.

### Agentic node lifecycle

A future `a3s-code` Agentic node will run as a dedicated, immutable Runtime
artifact rather than extending the current bounded Agent loop:

```text
Workflow step
  -> A3S Runtime unit
  -> pinned A3S Code Agent / AgentDir
  -> workspace + tools + Skills + MCP
  -> streamed events + atomic session checkpoint
  -> structured result + workspace patch + verification
```

Session state and checkpoints will be externalized through signed control-plane
references so another Runtime replica can resume an interrupted node. Internal
Agent confirmation will become a durable Flow interaction instead of holding a
Runtime unit open.

## Runtime contract

Every invocation carries stable workflow, run, step, node, attempt, and phase
identity plus an immutable runner artifact. A provider receives requested
resources, isolation, network policy, secret references, and a bounded output
artifact contract. The worker accepts a result only when generation, media
type, size, and SHA-256 digest match.

| Node | Runtime behavior |
| --- | --- |
| Start | Materializes typed workflow input |
| Template | Performs typed JSON token substitution |
| LLM | Calls an OpenAI-compatible gateway |
| Agent | Runs the currently shipped bounded model/tool loop |
| Tool / HTTP | Calls an allow-listed endpoint |
| Router | Selects one named source handle |
| Memory | Stores or searches A3S Memory backed by PostgreSQL |
| Approval | Executes, suspends as a durable hook, then resumes through Runtime |
| Output | Produces the final typed result through Runtime |

Placement is part of every node:

```json
{
  "provider": "production",
  "pool": "gpu-a100",
  "cpuMillis": 2000,
  "memoryBytes": 4294967296,
  "pids": 256,
  "timeoutMs": 120000,
  "isolation": "container",
  "network": "outbound",
  "secrets": [
    {
      "name": "model-api-key",
      "reference": "env://MODEL_API_KEY",
      "target": {
        "kind": "environment",
        "variable": "MODEL_API_KEY"
      }
    }
  ]
}
```

The bundled process provider is a local-development adapter. It implements the
A3S Runtime lifecycle and evidence contract, but does not claim container,
microVM, cgroup, or confidential-computing enforcement. Production providers
must reject policies they cannot enforce.

## Manual deployment

### Docker Compose

```bash
docker compose up --build
```

Open <http://localhost:3000>. The API is available at
<http://localhost:8080/api/health>.

Scale the durable scheduler separately from the API:

```bash
docker compose up --build --scale worker=3
```

### From source

Prerequisites: Rust 1.88+, Bun 1.3.14, and PostgreSQL 16+.

```bash
cargo build --workspace

export A3S_WORKFLOW_DATABASE_URL="postgres://workflow:workflow@127.0.0.1:5432/workflow"
export A3S_NODE_ARTIFACT_URI="file://$(pwd)/target/debug/a3s-workflow-node"
export A3S_NODE_ARTIFACT_DIGEST="sha256:$(sha256sum target/debug/a3s-workflow-node | cut -d' ' -f1)"
```

Start the Runtime provider, API, worker, and Studio independently:

```bash
target/debug/a3s-workflow-runtime-provider
target/debug/a3s-workflow-server
target/debug/worker

cd web
bun install --frozen-lockfile
bun run dev
```

See the [development guide](docs/development.md), including Windows
PowerShell instructions.

## Local end-to-end acceptance

Codex and other coding agents use the official
[A3S Test](https://github.com/A3S-Lab/Test) CLI locally. It drives the Studio
through semantic browser targets, saves the graph, starts a run, waits for the
typed output, and checks the Runtime record for every sample node.

```bash
scripts/e2e.sh
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\e2e.ps1 `
  -BrowserExecutable C:\path\to\agent-browser.exe
```

A3S Test is intentionally outside CI. Screenshots, accessibility snapshots,
console records, page errors, and evidence are kept local; the coding agent
consumes the exit code and scenario report.

## Technology

| Layer | Technology |
| --- | --- |
| Service composition and HTTP | [A3S Boot](https://github.com/A3S-Lab/Boot) `0.1.3` |
| Durable orchestration | [A3S Flow](https://github.com/A3S-Lab/Flow) `0.4.3` |
| Execution contract | [A3S Runtime](https://github.com/A3S-Lab/Runtime) `0.2.0` |
| Events, persistence, and memory | A3S Event, A3S ORM, A3S Memory |
| Source of truth | PostgreSQL |
| Studio | Bun `1.3.14`, Rsbuild `2.1.9`, React `19`, React Flow, Lucide |
| End-to-end acceptance | [A3S Test](https://github.com/A3S-Lab/Test) `0.4.3` |
| Coding-agent interface | Rust CLI and the `$a3s-workflow` Codex Skill |

Studio visual and interaction changes follow the shared
[A3S Web Design System](web/DESIGN.md).

## Repository map

```text
crates/workflow-protocol/  Stable node invocation and result protocol
cli/                       Machine-readable coding-agent CLI
node-runner/               Runtime-executed built-in node implementation
runtime-provider/          Development A3S Runtime HTTP provider
server/                    A3S Boot API and independently scalable Flow worker
web/                       Bun + Rsbuild + React Studio
skills/a3s-workflow/       Codex Skill and authoring reference
config/                    Service, Runtime, gateway, memory, and policy config
deploy/                    Container images and reverse proxy
scripts/install.*          Cross-platform install and deployment entrypoints
tests/e2e/                 Local A3S Test acceptance manifests
docs/                      Architecture, development, testing, and providers
```

## Verification

```bash
make verify
```

This runs Rust formatting, Clippy, workspace tests, Bun type checking, frontend
tests, and the production Studio build. `make e2e` runs local browser
acceptance against an already started stack. The complete case matrix lives in
[docs/testing.md](docs/testing.md).

## License

A3S Workflow is available under the [MIT License](LICENSE).
