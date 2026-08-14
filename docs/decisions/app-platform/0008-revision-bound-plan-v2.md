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

Compiler schema 2 Workflow revisions atomically own three mandatory immutable
canonical ACL children: exact step-to-descriptor bindings, the recoverable
descriptor registry snapshot containing exactly those revisions, and one
typed-variable contract. Decision 0011 permits one optional digest-bound
variable-default material child. The revision semantic digest includes the
binding, variable, and optional default-material digests, but excludes registry
presentation and admission metadata.

`cloud.workflow.plan.v2` copies every step's exact descriptor ID, SemVer, and
semantic digest and pins both the revision semantic-contract-set digest and the
variable-contract digest. Plan v1 remains readable, byte-stable, and bound to
Flow workflow version 1. Plan v2 uses WorkflowRun input/runtime/Flow version 2;
the immutable run input carries the exact canonical variable contract, and the
adapter reconstructs invocation, node-output, and deterministic run values
from that input plus existing A3S Flow step and hook history. Explicit reads are
the step's sole data authority and are exposed through `current`; legacy
`input` and `steps.*` tokens remain available only when the step has no reads.

## Consequences

Migration `103` adds only immutable children of `WorkflowRevision` and widens
the existing plan table to its paired v2 schema/compiler revision. Migration
`105` only raises the existing immutable WorkflowRun input bound so the already
bounded Plan, payloads, and variable ACL fit in one replay authority. Neither
migration adds a mutable node catalog, variable store, event log, scheduler,
queue, or runtime provider. Decision 0009's read-only discovery catalog may
publish candidate profiles, but publication always snapshots exact admitted
semantics into the revision.

The first runtime subset materializes invocation inputs, node outputs,
deterministically ordered run assignments, direct reads, and opaque Secret or
immutable-object references. Decision 0010 exposes the same materialization as
one authorized, Flow-derived read projection. Decision 0011 supplies exact
digest-bound defaults through immutable revision and Run input material.
Composite-local scopes and exports and Applications-owned reads/writes still
fail closed until their owning adapters exist.
