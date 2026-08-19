# 0013: Reuse Flow as the sole portable DAG structural compiler

Status: Accepted

## Context

Cloud's authoritative Workflow definition is closed A3S ACL. It owns product
node kinds, capability references, branch handles, ontology and variable
semantics, revision admission, and immutable Plan identity. A3S Flow now owns a
portable `WorkflowDag` model with generic identity, endpoint, scope, cycle,
deterministic-order, and semantic-digest compilation.

Cloud previously computed its own topological order. Keeping that algorithm
would create two structural compilers whose cycle behavior and ordering could
drift. Importing Flow's durable engine into the Workflow domain would create
the opposite error by leaking runtime, store, queue, and scheduler authority
into product semantics.

## Decision

Cloud maps its already parsed ACL `WorkflowSpec` programmatically into
`WorkflowDagNode` and `WorkflowDagEdge` values. It calls the single Flow
`WorkflowDag::execution_plan()` implementation for generic graph structure and
uses the returned top-level order when compiling a `PlanRevision`.

Cloud has no compatibility parser; A3S ACL remains its only product contract.
Cloud also does not use Flow's graph digest as business identity. Canonical
ACL, exact semantic child contracts, and the immutable Cloud Plan digest remain
authoritative.

After structural compilation, Cloud alone enforces exactly one input,
reachable output sinks, branch-handle uniqueness, capability compatibility,
typed dataflow, dominance, ontology, policy, and revision rules. Its
reachability and dominance analyses answer product-semantic questions; they do
not reimplement generic endpoint, scope, or cycle validation.

The Workflow domain may import exactly these three pure Flow types in
`workflow_graph.rs`: `WorkflowDag`, `WorkflowDagNode`, and `WorkflowDagEdge`.
A source architecture test rejects every other `a3s_flow` domain import and
continues to prohibit Flow engine, runtime, event-store, scheduler, worker,
queue, and command types. Those execution types remain confined to Cloud's
existing infrastructure and Operations integration.

The dependency target is one Flow revision per Cloud build. At acceptance,
`a3s-code-core` still brought Flow `0.11.0` transitively while Cloud pinned
Flow `0.13.1`. The Flow `1.0.0-rc.1` qualification closes that compatibility
debt: Cloud and Code `7.0.1` now resolve the same exact Flow revision. The
upgraded `F0` composition still requires re-certification; Cloud must not fork,
patch, or copy Code to hide future mismatches.

## Consequences

Cloud deletes its indegree calculation and topological sorter. Duplicate node
and edge identities, missing endpoints, self-edges, and cycles now fail through
Flow with a Cloud context prefix. Focused integration tests lock those
invariants and separate them from Cloud-owned branch and reachability tests.

Presentation tooling cannot become an execution authority or bypass Cloud ACL
publication. Adding another graph parser or structural compiler in
Applications, Workflow, an adapter, or a presentation package requires a
superseding decision.
