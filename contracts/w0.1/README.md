# W0.1 Workflow and Ontology contracts

These fixtures freeze the first Cloud-owned, closed A3S ACL boundary for
Workflow and Ontology semantics. They preserve the useful graph and validation
outcomes audited from standalone A3S Workflow commit
`2febfe30551e5d1bc7aa7f36f09aa6ee2a7f85c5` without importing its server,
queue, Runtime provider, node runner, Memory service, deployment stack, CLI
authority, legacy product-configuration authority, or Studio.

`workflow.acl` proves bounded deterministic DAG admission. `ontology.acl`
proves bounded object, relation, and rule admission. Both reject unknown fields
and produce canonical semantic SHA-256 digests through `a3s-acl`.

The source repository's recoverable Git history is retained in the Cloud
remote under `archive/workflow-standalone-20260807/branches/*` and
`archive/workflow-standalone-20260807/pulls/*`. The archive verification covers
29 source refs and all 36 commits reachable from the source branches and pull
request head/merge refs.

The ten standalone node outcomes map to the Cloud step kinds documented in
`docs/workflow-evolution-plan.md`. External steps bind only exact federated
capability references and are dispatched later through their owning Cloud
application ports.
