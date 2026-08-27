# 0070: Acquire one authorized trusted BuildPlan source layout

Status: Accepted

## Context

`P0.1-C1/C3` define and production-compose deterministic BuildPlan detection,
but the internal `DetectBuildPlanProposals` query still accepts a complete
`SourceLayoutSnapshot` from its caller. That value is canonical and bounded,
yet the handler cannot prove that its repository identity, commit, whole-tree
digest, paths, or inspected Asset ACL bytes came from the accepted Sources
revision. A later controller could therefore become an accidental source-byte
or checkout authority.

Sources already owns the only Git checkout implementation, private GitHub
installation credentials, immutable checkout receipt, whole-tree digest,
replay validation, and cleanup path. Artifacts already consumes that mechanism
through a Sources-owned archive adapter. Re-enumerating files or reimplementing
the public/private credential fallback for Developer Workflows would create a
second mechanism and two inconsistent integrity policies. The existing
SourceRevision resolver also performed the same organization-connection and
installation-token lookup inline, so adding that lookup directly to checkout
would preserve an older duplication inside Sources.

## Decision

`P0.1-C5` changes the internal detection query to accept only exact
Organization, Project, Environment, accepted `SourceRevisionId`, and Principal
identity. It reuses the existing Developer Workflows authorization port with a
closed `DetectBuildPlan` action before it asks Sources for revision or provider
evidence. Concealed or unauthorized Environments therefore cause no repository
lookup, credential issuance, or checkout.

Developer Workflows Application owns `IBuildPlanSourceLayoutPort` and its
minimal request/error language. A single Sources Infrastructure adapter
implements that consumer port. It:

1. queries the existing `ISourceBuildInputQueryPort` for the exact accepted
   revision and rechecks all four scope identities;
2. asks the Sources-owned `IAuthorizedSourceCheckout` for a unique transient
   checkout of the published canonical repository and commit;
3. maps the checkout's canonical file inventory into the existing bounded
   `SourceLayoutSnapshot`, retaining inspected bytes only for the fixed
   `.a3s/asset.acl` detector evidence;
4. invokes the distinct credential-free replay operation and rejects missing
   bytes or any receipt, inventory, content, or evidence drift; and
5. removes the owned transient checkout before returning either success or a
   mapped failure.

`GitSourceCheckout` remains the sole source-inventory traversal and whole-tree
digest authority. Its returned owner value now includes the per-file kind,
size, and content digest already calculated during that traversal. The layout
adapter never enumerates the checkout directory. `AuthorizedSourceCheckoutService`
is the sole public-first checkout coordinator and is shared by both the
existing Artifacts archive adapter and the new layout adapter. One
`SourceRepositoryCredentialService` now owns connection restoration,
authoritative-installation validation, token issuance, and error redaction for
both SourceRevision resolution and checkout fallback. The archive's existing
packaging walk only encodes an already validated checkout and does not define
source identity. Credentials and local paths never cross the Sources boundary.
A separate `replay` operation on both checkout ports can only revalidate an
already committed checkout; it cannot issue a credential, contact a provider,
or recreate missing bytes.
A newly committed checkout is removed if its first replay fails, and the
authorization service removes the requested owner checkout before rejecting an
inconsistent success receipt.

The application root constructs one repository credential service, one Git
checkout, one authorized checkout service, and one Sources build-input query
for every non-Relay process family.
Worker build staging and management detection share those instances in the
all-in-one role; dedicated roles acquire only the consumers they execute. The
existing detector set and CQRS registration remain singular.

C5 adds no public route, client, CLI, MCP tool, table, migration, aggregate,
event, Outbox, Relay, queue, worker, retry rail, BuildRun, Workload, Execution,
Route, Operation, scheduler, or owner lifecycle write.

## Consequences

- Production detection can no longer accept caller-authored source bytes or a
  caller-selected whole-tree digest.
- Authorization precedes all Sources and provider access, and every returned
  proposal remains bound to the exact accepted repository, commit, and
  checkout content digest.
- Source resolution and checkout use one repository credential authority;
  public and private checkout use one fallback policy, and the Artifacts build
  path no longer contains its own credential fallback.
- Source layout acquisition is transient and cleanup-bound. It does not become
  another cache, job, Inbox, queue, or lifecycle.
- Public Developer Workflow interfaces, pre-acceptance repository discovery,
  monorepo fan-out, Compose import, and downstream build/deployment handoffs
  remain later P0 slices.
