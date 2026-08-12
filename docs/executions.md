# Ephemeral Executions

A3S Cloud Executions are durable control-plane records for finite OCI tasks
whose artifact references contain no credentials. They provide the
product-level capability previously suggested by the Box-local Lambda
experiment while keeping provider lifecycle mechanics in A3S Runtime and A3S
Box.

## Boundary

Cloud owns:

- tenant identity, idempotency, desired state, placement, cancellation, and the
  public API;
- immutable project-scoped `ExecutionTemplate` revisions expressed only as
  canonical A3S ACL;
- the durable A3S Flow operation and recovery decisions;
- capability matching against the latest ready Fleet node; and
- terminal outcome publication after cleanup.

Runtime owns the provider-neutral Task contract and lifecycle. Box is the sole
node-local provider. Each node must select the concrete `microvm` or `sandbox`
backend explicitly through `box.isolation`; the shipped profile selects
MicroVM, and Cloud never requests an automatic fallback. A node advertises and
accepts `Confidential` executions only when its optional `box.sev_snp` ACL
block constructs the confidential Box driver. Hardware mode must pin the
expected launch measurement and reject debug mode; explicit simulation is
development-only evidence and does not qualify a node as hardware-certified.

Executions do not add another scheduler, node channel, provider adapter, log,
artifact, or Secret store.

## REST contract

All routes use the normal `/api/v1` prefix and response envelope.

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/organizations/{organization_id}/projects/{project_id}/execution-templates` | Publish an immutable ExecutionTemplate revision |
| `GET` | `/organizations/{organization_id}/projects/{project_id}/execution-templates` | List bounded template revisions |
| `GET` | `/organizations/{organization_id}/projects/{project_id}/execution-templates/{template_id}/revisions/{revision_id}` | Read one exact template revision |
| `POST` | `/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions` | Create an Execution |
| `GET` | `/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions` | List recent Executions |
| `GET` | `/organizations/{organization_id}/executions/{execution_id}` | Read authoritative state |
| `DELETE` | `/organizations/{organization_id}/executions/{execution_id}` | Request cancellation |

Mutations require an `idempotency-key` header and the `execution:write` scope.
Reads require an authenticated tenant principal. Create and cancel replay the
original resource for the same key and canonical request, and reject reuse with
different input.

Template publication also requires `execution:write` and an idempotency key.
It accepts a bounded canonical A3S ACL definition inside the normal transport
request and returns its immutable template ID, revision ID, canonical ACL, and
semantic digest. There is no mutable template row or update endpoint. The
maintained client, CLI, and Management MCP call the same application handlers;
they do not parse or store another template format.

Example ExecutionTemplate ACL:

```acl
execution_template "release-check" {
  schema = "cloud.execution-template.v1"
  description = "Run one bounded release check"

  artifact {
    uri = "oci://registry.example/tasks/release-check@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    media_type = "application/vnd.oci.image.manifest.v1+json"
  }

  process {
    command = ["/app/release-check"]
    args = ["verify"]
    working_directory = "/workspace"

    environment "MODE" {
      value = "workflow"
    }
  }

  resources {
    cpu_millis = 250
    memory_bytes = 134217728
    pids = 64
    ephemeral_storage_bytes = 16777216
    timeout_ms = 30000
  }
}
```

Example create body:

```json
{
  "artifact": {
    "uri": "oci://registry.example/tasks/echo@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "mediaType": "application/vnd.oci.image.manifest.v1+json"
  },
  "process": {
    "command": ["/app/echo"],
    "args": [],
    "workingDirectory": null,
    "environment": {}
  },
  "input": {
    "message": "hello"
  },
  "resources": {
    "cpuMillis": 250,
    "memoryBytes": 134217728,
    "pids": 64,
    "ephemeralStorageBytes": null,
    "timeoutMs": 5000
  }
}
```

The artifact URI must already be pinned to the matching SHA-256 digest. Input
is bounded to 16 KiB. Process environment keys beginning with
`A3S_EXECUTION_` are reserved by Cloud.

Input and process environment are desired state: Cloud persists them in the
Execution and its idempotency response. They must not contain credentials or
other secret material. Typed Secret references and output artifacts are
intentionally outside this initial Execution shape.

## Workflow finite-task binding

A Workflow `execution` step may reference only a capability whose owner is
`executions`, type is `execution_template`, revision is an exact UUID, and
capability is exactly `execution.run`. The plan must also bind one exact target
environment. The capability digest is the canonical ExecutionTemplate digest;
Workflow never copies or edits the template definition.

When A3S Flow exposes the authority-bound step hook, the Workflow coordinator
calls the Executions-owned `IWorkflowExecutionPort`. That port resolves the
exact revision, validates its digest and environment, materializes only the
schema-checked effective input, and creates or adopts the ordinary Execution.
The Execution persists the parent WorkflowRun, Plan revision and digest, step
ID and attempt, and template identity and digest. PostgreSQL foreign keys bind
all of those immutable authorities, while a unique step-attempt index prevents
duplicate children even across coordinator races.

The coordinator links the existing Execution Operation as the A3S Flow child;
it does not create a Workflow scheduler, worker queue, task store, or Runtime
provider. A terminal child resumes the parent with a digest-bound result.
Parent cancellation and timeout request the existing Execution cancellation
path and wait until cleanup makes every child terminal before settling the
parent Flow run. Restarted coordinators adopt the same child by
`(organization, workflow run, step, attempt)`.

## Lifecycle

```text
queued
  -> scheduled
  -> running
  -> cleanup_pending
  -> succeeded | failed

queued | scheduled | running
  -> cancelling
  -> cleanup_pending
  -> cancelled
```

Cloud compiles one deterministic Runtime Task with no network, mounts, Secrets,
outputs, or restart policy. It selects only a ready node advertising Task,
Sandbox, `NetworkMode::None`, the OCI media type, and every requested resource
control.

Every outcome is cleanup-first. A queued cancellation can complete without a
provider command. Once a Runtime unit may exist, Cloud dispatches a
generation-fenced `RuntimeRemove` and waits for exact absent/removal evidence
before making the Execution terminal. Reconciliation, command IDs, Runtime
specification digests, and node identity remain internal.

## Configuration

The closed `executions` ACL block controls reconciliation, command leases,
observation polling, convergence, and cleanup:

```acl
executions {
  reconcile_interval_ms = 1000
  command_ttl_ms = 900000
  observation_poll_ms = 1000
  convergence_timeout_ms = 600000
  cleanup_timeout_ms = 300000
}
```

The command TTL must cover the maximum accepted execution timeout so a valid
Task cannot outlive its leased apply command.
