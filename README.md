<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Workflow — durable orchestration where every node is an A3S Runtime unit">
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Workflow/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Workflow/actions/workflows/ci.yml/badge.svg"></a>
  <img alt="Rust 1.88+" src="https://img.shields.io/badge/Rust-1.88%2B-101118?logo=rust&logoColor=white">
  <img alt="Bun 1.3.14" src="https://img.shields.io/badge/Bun-1.3.14-101118?logo=bun&logoColor=white">
  <img alt="PostgreSQL" src="https://img.shields.io/badge/PostgreSQL-source%20of%20truth-2587f5?logo=postgresql&logoColor=white">
  <img alt="A3S Runtime" src="https://img.shields.io/badge/A3S%20Runtime-every%20node-7137d8">
</p>

<p align="center">
  <strong>Build durable AI graphs. Execute every node as an immutable Runtime unit.</strong><br>
  A PostgreSQL-native control plane powered by A3S Boot and A3S Flow, with a Bun + Rsbuild + React Studio.
</p>

<p align="center">
  <a href="#why-a3s-workflow">Why</a> ·
  <a href="#architecture">Architecture</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#runtime-contract">Runtime contract</a> ·
  <a href="#end-to-end-proof">E2E proof</a>
</p>

## Why A3S Workflow

AI workflows are not ordinary background jobs. Model calls can be expensive,
Agents can pause for tools or people, and one graph may need CPU, GPU, sandbox,
or confidential execution pools. A3S Workflow makes those concerns explicit:

| Product invariant | What it means |
| --- | --- |
| **AI-native graph** | LLM, Agent, tool, router, memory, approval, input, and output are first-class typed nodes. |
| **Every node is Runtime-native** | The API and Flow worker never execute node business logic in-process. Start and output nodes cross the same boundary too. |
| **PostgreSQL is authoritative** | Definitions, event history, queue leases, hooks, memory, and Runtime evidence survive process failure in one durable store. |
| **Stateless nodes scale independently** | Provider and pool placement decouple node capacity from API and worker replicas. |
| **Evidence, not hope** | Unit ID, generation, artifact digest, invocation digest, observation, and output digest are retained per attempt. |

### Why PostgreSQL instead of Redis?

The control plane needs transactions, ordered event streams, optimistic graph
versions, recoverable leases, queryable audit evidence, and durable approval
hooks. PostgreSQL already provides all of them. Redis would create a second
truth boundary and a new recovery protocol without removing PostgreSQL.

Redis may become an optional cache one day. It will not own workflow truth.

## Architecture

<p align="center">
  <img src="assets/readme/architecture.svg" width="100%" alt="React Studio, A3S Boot, A3S Flow, PostgreSQL, and independently scalable A3S Runtime provider pools">
</p>

The Studio writes a desired graph. The A3S Boot API validates and versions it.
A3S Flow appends run events and leases ready work through PostgreSQL. Each ready
node is converted to a digest-bound A3S Runtime specification; the selected
provider returns an observed result to the durable history.

```text
Studio ──> Boot API ──> A3S Flow ──> PostgreSQL
                              │
                              └──> A3S Runtime ──> provider / pool / unit
                                         │
                                         └──> verified result + evidence
```

- **API replicas** serve graph, run, memory, approval, and evidence endpoints.
- **Flow workers** scale independently and claim queue rows with `SKIP LOCKED`.
- **Runtime providers** own execution lifecycle, isolation, resources, secrets,
  networking, artifacts, and logs.
- **PostgreSQL** is the only required infrastructure dependency.

Read the detailed [architecture](docs/architecture.md) and
[provider contract](docs/runtime-providers.md).

## Runtime contract

Every node invocation carries stable workflow/run/node/attempt identity plus an
immutable node-runner artifact. A provider receives requested resources,
isolation, network policy, secret references, and an output artifact contract.
The worker accepts the result only when the generation, media type, size, and
SHA-256 digest match.

| Node | Runtime behavior |
| --- | --- |
| Input | Materializes typed workflow input |
| Template | Performs typed JSON token substitution |
| LLM | Calls an OpenAI-compatible gateway |
| Agent | Runs a bounded model/tool loop |
| Tool / HTTP | Calls an allow-listed endpoint |
| Router | Selects one named source handle |
| Memory | Stores or searches A3S Memory backed by PostgreSQL |
| Approval | Executes, suspends as a durable hook, then resumes through Runtime |
| Output | Produces the final typed result through Runtime |

Placement is part of each node:

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
      "name": "openai-api-key",
      "reference": "env://OPENAI_API_KEY",
      "target": {
        "kind": "environment",
        "variable": "OPENAI_API_KEY"
      }
    }
  ]
}
```

The bundled process provider is for local development and CI. It implements
the full A3S Runtime lifecycle and artifact evidence, but does **not** claim
container, microVM, cgroup, or confidential-computing enforcement. Production
providers must reject policies they cannot enforce.

## Quick start

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

Start these processes independently:

```bash
target/debug/a3s-workflow-runtime-provider
target/debug/a3s-workflow-server
target/debug/worker

cd web
bun install --frozen-lockfile
bun run dev
```

See the full [development guide](docs/development.md), including the Windows
PowerShell path.

## End-to-end proof

The merge gate uses the official
[A3S Test](https://github.com/A3S-Lab/Test) engine. It drives the Studio through
semantic browser targets, changes the input, saves the graph, starts the run,
and waits for the typed output. It then asserts `3/3` Runtime units and captures
a screenshot, accessibility tree, console messages, and page errors.

With the local stack running:

```bash
scripts/e2e.sh
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\e2e.ps1 `
  -BrowserExecutable C:\path\to\agent-browser.exe
```

CI pins A3S Test `0.4.3` and the admitted browser protocol. Evidence is stored
under `.a3s-test/runs/` and uploaded for every CI run.

## Technology stack

| Layer | Technology |
| --- | --- |
| Service composition and HTTP | [A3S Boot](https://github.com/A3S-Lab/Boot) `0.1.3` |
| Durable orchestration | [A3S Flow](https://github.com/A3S-Lab/Flow) `0.4.3` |
| Execution contract | [A3S Runtime](https://github.com/A3S-Lab/Runtime) `0.2.0` |
| Events / persistence / memory | A3S Event, A3S ORM, A3S Memory |
| Source of truth | PostgreSQL |
| Studio | Bun `1.3.14`, Rsbuild `2.1.9`, React `19`, React Flow |
| End-to-end testing | [A3S Test](https://github.com/A3S-Lab/Test) `0.4.3` |

## Repository map

```text
crates/workflow-protocol/  Stable node invocation and result protocol
node-runner/               Runtime-executed implementation of every node kind
runtime-provider/          Development A3S Runtime HTTP provider
server/                    A3S Boot API and independently scalable Flow worker
web/                       Bun + Rsbuild + React Studio
config/                    ACL service, Runtime, gateway, memory, and policy config
deploy/                    Container images and reverse proxy
tests/e2e/                 Official a3s-test ACL manifests
docs/                      Architecture, development, and provider guidance
```

## Verification

```bash
make verify
```

This runs Rust formatting, Clippy, workspace tests, Bun type checking, frontend
tests, and the production Studio build. `make e2e` runs the browser merge gate
against an already running local stack.

## License

This repository is distributed under the terms in [LICENSE](LICENSE).
