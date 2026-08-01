# Development

## Prerequisites

- Rust 1.88 or newer
- Bun 1.3.14
- PostgreSQL 16 or newer
- `a3s-test` 0.4.3 and a protocol-compatible browser for end-to-end tests

## One-command installation

The repository installers build the CLI, install the Codex Skill, start the
Docker Compose deployment, and wait for `/api/health`:

```bash
./scripts/install.sh
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\install.ps1
```

Run either installer with its dry-run option before changing custom paths. Use
`--help` on macOS/Linux; inspect `Get-Help .\scripts\install.ps1 -Detailed` or
the script parameters on Windows.

Create the local database:

```sql
CREATE USER workflow WITH PASSWORD 'workflow';
CREATE DATABASE workflow OWNER workflow;
```

Build the workspace and compute the immutable node-runner digest:

```bash
cargo build --workspace
export A3S_NODE_ARTIFACT_URI="file://$(pwd)/target/debug/a3s-workflow-node"
export A3S_NODE_ARTIFACT_DIGEST="sha256:$(sha256sum target/debug/a3s-workflow-node | cut -d' ' -f1)"
export A3S_WORKFLOW_DATABASE_URL="postgres://workflow:workflow@127.0.0.1:5432/workflow"
```

Start the independently deployable processes in separate terminals:

```bash
target/debug/a3s-workflow-runtime-provider
target/debug/a3s-workflow-server
target/debug/worker

cd web
bun install --frozen-lockfile
bun run dev
```

Open <http://127.0.0.1:3000>. The API listens on `8080` and the local Runtime
provider on `8090`.

## Coding-agent CLI and Skill

The `a3s-workflow` CLI emits JSON and supports health checks, node discovery,
workflow apply/get/list, run start/get/wait/evidence, and approval resume. Set
`A3S_WORKFLOW_URL` and, when required, `A3S_WORKFLOW_API_TOKEN`.

```bash
a3s-workflow health
a3s-workflow node-types
a3s-workflow run evidence RUN_ID
```

The install scripts place the repository Skill at
`${CODEX_HOME:-~/.codex}/skills/a3s-workflow`. Its authoring reference documents
all ten node configurations and the provider/pool placement contract.

## Coverage

Install `cargo-llvm-cov` and reproduce the CI threshold with:

```bash
cargo llvm-cov --workspace --all-targets --locked --fail-under-lines 90
cd web && bun run test:coverage
```

Set `A3S_WORKFLOW_TEST_DATABASE_URL` to a disposable PostgreSQL database to run
the repository, memory, and Runtime evidence integration cases. CI always sets
this value; local test runs without it skip only those database-backed cases.
Both Rust and Studio source lines must remain at or above 90%. Studio coverage
excludes test files themselves, so test code cannot inflate the gate.

## End-to-end test

A3S Test is a local acceptance tool for Codex and other coding agents; it does
not run in GitHub Actions. With the four processes running, set
`A3S_TEST_BROWSER` if the compatible browser executable is not on `PATH`, then
run:

```bash
scripts/e2e.sh
```

On PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\e2e.ps1 `
  -BrowserExecutable C:\path\to\agent-browser.exe
```

The manifest drives the Studio, saves the graph, submits a run, and asserts
that all three sample nodes report Runtime evidence before checking the typed
output. It does not request screenshots, accessibility data, console output,
page-error captures, or upload test evidence. The coding agent consumes the
step report locally.
