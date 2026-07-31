# Development

## Prerequisites

- Rust 1.88 or newer
- Bun 1.3.14
- PostgreSQL 16 or newer
- `a3s-test` 0.4.3 and a protocol-compatible browser for end-to-end tests

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

## End-to-end test

With the four processes running, set `A3S_TEST_BROWSER` if the compatible
browser executable is not on `PATH`, then run:

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
output. Screenshots, accessibility data, console output, and page errors are
written under `.a3s-test/runs/`.
