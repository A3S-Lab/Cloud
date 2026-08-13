# 0008: Revision-bound semantic contracts and Plan v2

Status: Accepted

## Context

The descriptor registry and typed-variable contracts were canonical domain
contracts, but `cloud.workflow.plan.v1` had no descriptor identity or variable
contract digest. Treating a mutable registry as plan authority would make old
runs change under catalog updates. Mixing these contracts into executable
payloads would also copy catalog material into Flow history and weaken the
existing exact-payload invariant.

## Decision

Compiler schema 2 Workflow revisions atomically own three immutable canonical
ACL children: exact step-to-descriptor bindings, the recoverable descriptor
registry snapshot containing exactly those revisions, and one typed-variable
contract. The revision semantic digest includes the binding and variable
digests, but excludes registry presentation and admission metadata.

`cloud.workflow.plan.v2` copies every step's exact descriptor ID, SemVer, and
semantic digest and pins both the revision semantic-contract-set digest and the
variable-contract digest. Plan v1 remains readable and byte-stable. Plan v2
execution fails closed until the typed-variable adapter projects values through
the existing WorkflowRun input and A3S Flow history.

## Consequences

Migration `103` adds only immutable children of `WorkflowRevision` and widens
the existing plan table to its paired v2 schema/compiler revision. It does not
add a mutable node catalog, variable store, event log, scheduler, queue, or
runtime provider. A future discovery catalog may publish candidate descriptors,
but publication always snapshots exact admitted semantics into the revision.
