# Test strategy

A3S Workflow treats tests as proof of its product invariants: PostgreSQL owns
durable state, every node crosses the A3S Runtime boundary, and provider/pool
placement allows stateless node capacity to scale independently. Coverage is a
quality gate, not a substitute for those behavioral checks.

## Coverage gates

| Surface | Command | Gate |
| --- | --- | ---: |
| Rust workspace | `cargo llvm-cov --workspace --all-targets --locked --fail-under-lines 90` | 90% source lines |
| Studio | `cd web && bun run test:coverage` | 90% source lines and functions |

Rust coverage must run with `A3S_WORKFLOW_TEST_DATABASE_URL` pointed at a
disposable PostgreSQL database so durable repository, memory, invocation, and
Runtime-evidence paths are measured. Bun excludes `*.test.*` files from its
totals, preventing tests from covering themselves.

## Required case matrix

| Boundary | Happy path | Failure and recovery path | Primary test layer |
| --- | --- | --- | --- |
| Graph definition | Valid DAG, deterministic ordering, named router edges | Duplicate IDs, dangling edges, cycles, disconnected branches, invalid boundaries | Rust unit |
| All node kinds | Start, Template, LLM, Agent, Tool, Router, Memory, HTTP, Approval, Output return typed results | Invalid schemas/config, unsafe HTTP, bounded Agent loop, missing approval suspension | Node runner unit |
| Runtime placement | Per-node provider, pool, resources, isolation, network, environment and Secret references | Unknown provider, duplicate Secret, unsupported policy/capability | Rust integration |
| Flow replay | Start-to-output, selected router branch, approval execute/hook/resume | Unknown route, disposed hook, stalled graph, malformed durable result | Rust integration |
| Runtime lifecycle | Apply, inspect, logs, stop, remove and artifact download | Authentication, malformed request, missing unit, unsupported exec, corrupt/oversized artifact | Provider contract integration |
| PostgreSQL | Workflow versions, events, queue leases, hooks, memory and execution evidence survive replay | Optimistic conflicts, redelivery mismatch, invalid token, cancellation | PostgreSQL integration |
| HTTP API and CLI | Health, catalog, workflow CRUD, run/history/evidence, memory, approvals | Validation, unauthorized internal invocation, missing resource, API error propagation | Boot application + CLI tests |
| Studio | Author/delete/connect/save/run, Runtime policy editing, evidence display, approval resume, responsive dock | Duplicate boundaries, invalid JSON, save/poll/approval errors, removed non-core shell stays absent | Bun component/integration |
| Product acceptance | Load sample graph, run all sample nodes through Runtime, assert typed output and completed unit count | Browser console/page-error and accessibility diagnostics stay local for investigation | Local A3S Test |

## A3S Test policy

`a3s-test` is a local acceptance tool for Codex and other coding agents. It is
not a GitHub Actions job and its screenshots, accessibility snapshots, console
records, and page errors are never uploaded. Run it only against an explicitly
started local stack:

```bash
scripts/e2e.sh
```

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\e2e.ps1 `
  -BrowserExecutable C:\path\to\agent-browser.exe
```

The authoritative pass condition is the `a3s-test run` exit code and scenario
step report. Files under `.a3s-test/runs/` are ignored, disposable diagnostics.
