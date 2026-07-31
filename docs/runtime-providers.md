# Runtime providers

The control plane selects a provider from `node.data.runtime.provider` and an
optional pool from `node.data.runtime.pool`. Providers are configured in
`config/workflow.acl`; each entry exposes the standard A3S Runtime HTTP
lifecycle used by `RuntimeClient`.

Example placement policy:

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

Without a pool, the provider selector is the `provider` value. With a pool,
the selector is `<provider>-<pool>`; the example therefore requires a
`production-gpu-a100` Runtime entry in `config/workflow.acl`.

Provider implementations must:

1. verify every input and executable artifact digest;
2. preserve unit ID and generation semantics;
3. reject isolation, networking, resource, or secret policies they cannot
   enforce;
4. publish a bounded output artifact with a SHA-256 digest;
5. return enough observation data for durable audit;
6. make retries idempotent for the submitted unit/generation.

Stateless nodes are safe to spread across provider replicas because durable
workflow state is never stored in the node process. Providers that need local
lifecycle state must use a shared or sharded Runtime state store before they
are horizontally scaled.
