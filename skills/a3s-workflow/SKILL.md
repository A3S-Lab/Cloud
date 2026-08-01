---
name: a3s-workflow
description: Author, deploy, run, monitor, debug, and locally verify A3S Workflow AI-native workflow graphs through the a3s-workflow coding-agent CLI and A3S Test. Use when an agent needs to create or update workflow JSON, choose per-node A3S Runtime provider and pool placement, start or wait for runs, inspect Runtime evidence, resume approvals, diagnose PostgreSQL-backed execution, run local end-to-end acceptance, or install and deploy A3S Workflow.
---

# A3S Workflow

Operate A3S Workflow through its machine-readable CLI. Preserve the control-plane and Runtime boundaries while authoring or debugging graphs.

## Preflight

1. Run `a3s-workflow health`.
2. Run `a3s-workflow node-types` before generating node configuration.
3. If the CLI is unavailable from a repository checkout, run `scripts/install.sh` on macOS/Linux or `scripts/install.ps1` on Windows.
4. Set `A3S_WORKFLOW_URL` when the control plane is not at `http://127.0.0.1:8080`.
5. Set `A3S_WORKFLOW_API_TOKEN` only when the deployment requires a bearer token.

## Preserve the execution contract

- Route every node kind through A3S Runtime, including start, router, approval execute/resume, and output.
- Never move node business logic into the API or Flow worker.
- Treat PostgreSQL as the only durable source of truth. Do not add Redis, local files, or process memory as authoritative workflow state.
- Put specialized or stateless work on an explicit `runtime.provider` and `runtime.pool`. The selector is `<provider>-<pool>` when a pool is present.
- Keep secrets as references. Never embed secret values in workflow JSON.
- Require digest-bound Runtime output and inspect node execution evidence after failures.

Read [references/workflow-authoring.md](references/workflow-authoring.md) before creating or changing a graph.

## Author and apply a workflow

1. Discover supported nodes with `a3s-workflow node-types`.
2. Create a JSON draft with `name`, `description`, `nodes`, and `edges`. Include `version` and `id` when updating an existing workflow.
3. Give each node an intentional Runtime policy. Use distinct pools for independently scalable workloads such as agents, model calls, tools, and HTTP tasks.
4. Validate graph invariants from the authoring reference.
5. Apply the graph:

```bash
a3s-workflow workflow apply workflow.json
```

Do not write workflow rows directly to PostgreSQL.

## Run and observe

Start with inline JSON or an `@file` payload:

```bash
a3s-workflow run start WORKFLOW_ID --input '{"task":"fix the failing tests"}'
a3s-workflow run start WORKFLOW_ID --input @input.json
```

Capture the returned `runId`, then wait and inspect Runtime evidence:

```bash
a3s-workflow run wait RUN_ID --timeout-seconds 600 --poll-ms 500
a3s-workflow run evidence RUN_ID
```

Treat a terminal `failed` or `cancelled` status as a failed task even though the HTTP request succeeded. Use evidence to report the selected provider, pool, immutable unit ID, generation, spec digest, output digest, and provider failure.

## Resume approvals

Inspect the run before approving. Resume only the requested approval node:

```bash
a3s-workflow run approve RUN_ID APPROVAL_NODE_ID \
  --payload '{"approved":true,"actor":"coding-agent"}'
a3s-workflow run wait RUN_ID
```

Approval resume is another Runtime execution; require fresh evidence for it.

## Diagnose failures

1. Run `a3s-workflow run get RUN_ID` and `a3s-workflow run evidence RUN_ID`.
2. Confirm every scheduled node has Runtime evidence and the expected provider/pool selector.
3. Distinguish graph validation, provider selection, Runtime protocol, node execution, and output digest failures.
4. Fix the workflow or provider configuration, apply a new workflow version, and start a new run.
5. Do not mutate durable run history or retry a node outside A3S Flow.

## Verify observable changes locally

A3S Test is a coding-agent-local acceptance tool. Do not add it to CI and do
not upload `.a3s-test` browser diagnostics.

With the local PostgreSQL, Runtime provider, API, worker, and Studio running,
execute `scripts/e2e.sh` on macOS/Linux or `scripts/e2e.ps1` on Windows. Treat a
non-zero A3S Test exit code as a failed task. Inspect `.a3s-test/runs/` locally
when diagnosing browser, console, accessibility, or Runtime-evidence failures.

Use `--compact` for one-line JSON in scripts. Preserve full JSON evidence in automated reports.
