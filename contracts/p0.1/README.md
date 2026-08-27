# P0.1 developer BuildPlan contract

`build-plan.acl` freezes the component-only `P0.1-C1` proposal emitted from an
exact source-layout snapshot. The canonical ACL binds the repository/commit
identity digest, accepted checkout content digest, detector revision, evidence
file, project root, and existing Sources-owned Dockerfile `BuildRecipe`.

`accepted-build-plan.acl` freezes the component-only `P0.1-C2` acceptance
contract. `a3s.cloud.build-plan.v1` embeds the exact canonical proposal and adds
the Sources-owned `SourceRevisionId`; the proposal and accepted-plan digests are
therefore independent of checkout directory, caller, acceptance time, or
storage adapter. Caller and time remain immutable record/audit facts outside
the desired-state ACL.

`P0.1-C5` changes no frozen ACL schema. It production-binds detection to one
authorized, exact accepted `SourceRevision`: Developer Workflows requests the
layout through its consumer-owned port, while Sources alone resolves provider
credentials through the same repository-credential authority used by
SourceRevision resolution, then traverses and digests the Git checkout, replays
its immutable receipt through a strict credential-free operation that cannot
recreate missing bytes, and removes the transient checkout. Only the existing
bounded `SourceLayoutSnapshot` enters detection; credentials, receipts, and local paths
remain outside this contract.

`P0.1-C6` also changes no frozen ACL schema. It exposes the existing detection
query and acceptance command plus authorized accepted-plan get/list queries
through REST, the versioned OpenAPI contract, maintained client and CLI, and
four Management MCP tools. Every adapter dispatches the same CQRS boundary.
Requests accept only exact identities and canonical `proposalAcl`; responses
preserve canonical proposal/contract ACLs, typed digests, evidence, and immutable
acceptance facts without source bytes, credentials, checkout receipts, or local
paths. The Application query service alone authorizes reads and rejects
repository scope, validity, page-bound, duplicate, or canonical-order drift.

The C1 proposal remains review evidence only. C2 persists one immutable
acceptance per Source revision and project root through migration `146`, with
authorization-first internal CQRS, exact Sources evidence admission,
idempotency, audit, and Outbox. C6 exposes that authority but does not accept a
Source revision, start a BuildRun, create a Workload, publish a Route, or own a
deployment scheduler. Later P0 slices must pass the accepted plan through those
existing owning contexts.
