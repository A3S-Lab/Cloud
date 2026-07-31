# Workflow authoring reference

## Graph envelope

Create:

```json
{
  "name": "Coding agent",
  "description": "Plans, approves, and applies a bounded code change",
  "nodes": [],
  "edges": []
}
```

For updates, include the server-provided `id` and `version`. The CLI removes server-owned timestamps before sending the update.

Each node has this shape:

```json
{
  "id": "agent",
  "type": "agent",
  "position": { "x": 320, "y": 120 },
  "data": {
    "label": "Coding agent",
    "config": {},
    "runtime": { "secrets": [] }
  }
}
```

Edges contain `id`, `source`, and `target`. Router edges also require a unique `sourceHandle` matching a configured route.

## Runtime placement

Every node is an immutable A3S Runtime Task unit. Use a policy such as:

```json
{
  "provider": "production",
  "pool": "coding-agents",
  "cpuMillis": 2000,
  "memoryBytes": 4294967296,
  "pids": 256,
  "timeoutMs": 600000,
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

Supported isolation values are `process`, `container`, `sandbox`, and `confidential`. Supported network values are `none` and `outbound`.

Without `pool`, the selector is the provider. With `pool`, it is `<provider>-<pool>`; configure that exact selector under `runtimes` in `config/workflow.acl`. Scale API replicas, Flow workers, and Runtime provider pools independently. A horizontally scaled provider must use shared or sharded lifecycle state.

## Node configurations

All ten types cross the Runtime boundary:

| Type | Minimal configuration |
| --- | --- |
| `start` | `{}`; emits typed workflow input |
| `template` | `{"value": ...}` or `{"template":"Hello {{input.name}}"}` |
| `llm` | `{"prompt":"...","model":"optional"}` |
| `agent` | `{"prompt":"...","maxIterations":6,"tools":[]}` |
| `tool` | `{"url":"https://allowed.example/tool","method":"POST","body":{}}` |
| `router` | `{"routes":[{"when":{"value":"{{input.kind}}","equals":"fix"},"route":"fix"}],"default":"other"}` |
| `memory` | `{"operation":"search"}`; operations are `store`, `search`, `retrieve`, and `delete` |
| `http` | `{"url":"https://allowed.example/api","method":"GET"}` |
| `approval` | `{"message":"Apply this patch?","details":{}}` |
| `output` | `{}` or `{"value": ...}` |

Agent tools use an OpenAI-compatible function envelope plus an execution endpoint:

```json
{
  "function": {
    "name": "run_tests",
    "description": "Run the repository test suite",
    "parameters": {
      "type": "object",
      "properties": {
        "scope": { "type": "string" }
      }
    }
  },
  "endpoint": "https://tools.example.test/run-tests",
  "method": "POST"
}
```

HTTP and tool hosts must be present in `security.http_allowed_hosts`. Wildcards match subdomains only; `*.example.test` does not match the bare `example.test` host.

## Template tokens

- `{{input}}` or `{{input.path.to.value}}`
- `{{steps.node_id}}` or `{{steps.node_id.path}}`
- Array indexes use numeric path segments, for example `{{input.files.0.path}}`.
- A whole-token value preserves its JSON type. Interpolation inside surrounding text converts the value to text.

## Graph invariants

- Use exactly one `start` and one `output` node.
- Use 1-96 ASCII letters, numbers, hyphens, or underscores for node and edge IDs.
- Give every node a nonblank label.
- Keep the graph acyclic.
- Make every node reachable from start and able to reach output.
- Do not add incoming edges to start or outgoing edges from output.
- Give every non-start node an upstream edge and every non-output node an outgoing edge.
- Give every router edge a unique `sourceHandle`.
