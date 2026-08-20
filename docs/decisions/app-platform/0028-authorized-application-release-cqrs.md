# 0028: Authorize Application release CQRS before replay

Status: Accepted

## Context

`APP0.1-C1` owns the immutable Application release contract and C2 persists
that authority atomically. The next component slice needs commands and queries
without turning Applications into another Workflow, execution, session,
provider, or delivery system. It must also prevent idempotency receipts from
leaking a previously authorized result after a caller loses project access.

Resolving a mutable Workflow head during replay would make the same accepted
request depend on later external state. Copying the Workflow graph or semantic
contracts into Applications would create a second Workflow authority.

## Decision

Application commands and queries use Identity's existing
`ResourceAccessEvaluator` for the canonical project before parsing mutation
input, reading an idempotency receipt, or calling a repository. Denial is
reported as the same not-found boundary used for absent project resources.

Create and publish commands derive a canonical idempotency request. A matching
receipt reconstructs the exact historical Application head at its immutable
release and returns before Workflow resolution. A new write resolves one exact
Workflow definition/revision through a metadata-only port over the existing
Workflow repository, then matches contract, payload-set, semantic-contract-set,
input-schema, and output-schema digests. The v1 Application contract has one
output-schema digest, so its publication admission requires exactly one
Workflow Output step; Workflow retains its broader multi-output authority.

The handlers call the C2 `IApplicationRepository` and therefore reuse its one
A3S ORM transaction for the release, head, idempotency, audit, and Outbox
facts. Queries use the same tenant/project-filtered repository and bounded list
limits. This slice adds no REST route, client, CLI, Management MCP tool, graph,
Flow history, queue, worker, session, provider, credential, or Gateway route.

## Consequences

- Revoked project access is checked before replay and cannot reveal an earlier
  result.
- A valid replay is stable after the Workflow head or Application head moves.
- New publication fails closed on missing, cross-project, non-semantic, or
  digest-drifted Workflow evidence.
- Applications retains only immutable Workflow identity and digest evidence;
  Workflow and Flow keep semantic and execution authority.
- Production composition and maintained public management interfaces remain a
  later `APP0.1` slice, so C3 is not an availability claim.
