# 0009: Read-only Built-in Workflow Node Catalog Projection

Status: Accepted

## Context

The application-platform parity manifest already freezes the accepted built-in
node inventory, owning gates, dependencies, evidence, and availability. The
WorkflowRevision descriptor registry separately freezes the exact executable
semantics admitted by one revision. Product discovery needs a stable view of
the 23-node baseline without turning either contract into a mutable global
registry or adding another execution authority.

## Decision

`contracts/app-platform/v1/parity-manifest.acl` remains the sole source of
acceptance owner, gate, dependencies, evidence, and availability. The exact
23-node `workflow-node-profiles.acl` contract adds only coarse Workflow kind,
execution class, and semantic profiles, and binds the canonical parity-manifest
digest so the two files cannot drift.

Cloud composes both checked-in ACL contracts into one deterministic,
project-authorized read projection. REST contract `1.31.0`, the maintained
client, `workflow-nodes list`, and
`a3s_cloud_workflow_node_catalog_get` all call the same Workflow query. The
projection has no table, migration, index, cache, synchronizer, worker, or
write API.

Catalog presence never admits a descriptor for compilation or execution.
Only the exact immutable descriptor registry snapshot owned by a
WorkflowRevision can do that. `internal` means an owning implementation slice
exists; it does not mean public publication or arbitrary descriptor admission.
The catalog retains `parityClaim = false` until the composite public gate is
actually verified.

## Consequences

The catalog can expose the full accepted inventory while five Workflow-local
nodes are internal and the other eighteen remain unavailable. Code remains
Executions-owned, HTTP Request remains Connectors-owned, and invocation-only
triggers have no Flow step kind. A future inventory, owner, or availability
change must update the parity manifest and its digest-bound profile set
together. A future public claim also requires the owning gate and evidence;
neither presentation metadata nor catalog discovery can widen execution.

A3S Flow remains unchanged and continues as the sole durable orchestration
authority. Cloud adds no node scheduler, executor, command, variable store, or
parallel history mechanism.
