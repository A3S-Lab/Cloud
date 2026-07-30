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
- the durable A3S Flow operation and recovery decisions;
- capability matching against the latest ready Fleet node; and
- terminal outcome publication after cleanup.

Runtime owns the provider-neutral Task contract and lifecycle. Box is the sole
node-local provider. Each node must select the concrete `microvm` or `sandbox`
backend explicitly through `box.isolation`; the shipped profile selects
MicroVM, and Cloud never requests an automatic fallback.

Executions do not add another scheduler, node channel, provider adapter, log,
artifact, or Secret store.

## REST contract

All routes use the normal `/api/v1` prefix and response envelope.

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions` | Create an Execution |
| `GET` | `/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/executions` | List recent Executions |
| `GET` | `/organizations/{organization_id}/executions/{execution_id}` | Read authoritative state |
| `DELETE` | `/organizations/{organization_id}/executions/{execution_id}` | Request cancellation |

Mutations require an `idempotency-key` header and the `execution:write` scope.
Reads require an authenticated tenant principal. Create and cancel replay the
original resource for the same key and canonical request, and reject reuse with
different input.

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
