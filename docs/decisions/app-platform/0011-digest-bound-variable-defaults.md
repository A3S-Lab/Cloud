# 0011: Bind Workflow defaults as immutable revision material

Status: Accepted

## Context

The typed variable contract intentionally records only a
`default_value_digest`. A digest is sufficient for compilation and admission,
but it cannot reconstruct a missing invocation field or initialize run-local
state after restart. Accepting the digest without exact bytes would make replay
depend on mutable authoring state. Adding a variable-value table, cache, or
event log would duplicate A3S Flow's durable run authority.

## Decision

A compiler-schema-2 Workflow revision may own one optional canonical
`cloud.workflow.variable-defaults.v1` ACL child in addition to its three
mandatory semantic contracts. Each entry contains bounded canonical JSON and
its SHA-256 digest. The default-set identity and revision must match the typed
variable contract, and its entries must exactly cover declarations that carry
`default_value_digest`. Missing, extra, mistyped, non-canonical, or
digest-mismatched material fails closed.

The semantic-contract-set digest includes the optional default-set digest.
WorkflowRun compilation copies the exact ACL and digest into immutable Run v2
input. The shared execution/inspection materializer uses a default only when a
declared source value is absent, then applies the existing deterministic
assignments from Flow-observed step outputs. Opaque references, node outputs, required
declarations, and Applications-owned variables retain their existing default
prohibitions.

## Consequences

Migration `107` permits the optional fourth immutable revision child and
widens only the existing WorkflowRun input bound. REST/OpenAPI `1.34.0`, the
maintained client, CLI publication files, and Workflow Management MCP inputs
accept optional `variableDefaultsAcl`. Existing revisions with three semantic
children and Run v2 histories without default material remain byte-compatible.
Pre-107 revisions that declared only a default digest remain readable, but a
new Run still fails closed because the historical revision cannot reconstruct
the missing bytes; authors publish a successor revision with exact material.
No mutable variable table, event history, cache, worker, scheduler, queue, or
Flow change is introduced.
