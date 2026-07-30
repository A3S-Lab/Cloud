# MCP0.1 cross-repository fixture

This directory is the Cloud-owned producer fixture for the first hosted modern
MCP contract. It is not a production-readiness claim.

- Protocol revision: `2026-07-28`
- Projection schema: `a3s.cloud.mcp-gateway-projection.v1`
- Runtime wire schema: `a3s.runtime.unit-spec.v2`
- Gateway transport: one POST message, JSON or request-scoped SSE
- Protocol sessions, sticky affinity, GET streams, DELETE sessions, and
  post-dispatch replay: forbidden

The immutable profile digest is
`sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`.
It binds hosted-server behavior, not an AssetRelease. The mutable route policy
and exact target set are bound by the complete Gateway snapshot digest.

## Frozen files

| File | Consumer | SHA-256 of exact fixture bytes |
| --- | --- | --- |
| [`mcp-policy.acl`](mcp-policy.acl) | Cloud compiler and Gateway MCP policy parser | `5f30512ff696a7bbc25417819c2432027de20123f229d8ddbd29298d0da821e0` |
| [`runtime-unit-spec.json`](runtime-unit-spec.json) | Runtime and Box generic Service substrate | `5915c0ccac040fc4270ee5095de58b9115caee6e240464863cd6c3c1dcd59d23` |
| [`gateway-snapshot.acl`](gateway-snapshot.acl) | Gateway strict snapshot parser and fail-closed route ownership | `a3c12ad36e8c2c06787ec1b42899fa5cea5a10f00ce2ab42c1abaddec50036a5` |

The three implementation worktrees started from these exact upstream
revisions:

| Repository | Baseline revision |
| --- | --- |
| Runtime | `42e8884065eb98761098d59fe85c7d2433cf1207` |
| Cloud | `493b6cf59fc2ff00a6d60f42c7450194ec8bde44` |
| Gateway | `5026dffed0a80fb204c768bd4b1fe90014ed360e` |

Release commit revisions remain intentionally unset until the changes are
reviewed and committed. `MCP0.5` must replace the baseline table with the exact
committed Runtime, Box, Cloud, and Gateway revisions used by the joint run.
