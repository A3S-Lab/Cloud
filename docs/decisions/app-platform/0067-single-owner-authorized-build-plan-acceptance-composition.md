# 0067: Compose one owner-authorized BuildPlan acceptance path

Status: Accepted

## Context

`P0.1-C2` defines the immutable BuildPlan acceptance command, its
Developer Workflows-owned authorization and Sources evidence ports, the sole
BuildPlan repository, migration `146`, audit record, and transactional Outbox
fact. `P0.1-C3` production-composes only deterministic proposal detection.
The acceptance components still had no production command registration or
concrete authorization composition.

Rebuilding membership or Resource Grant policy in the command handler would
create another Identity authorization mechanism. Trusting caller-supplied
Project, Environment, or Source evidence would similarly create shadow
Projects and Sources authorities. Passing Identity policy types into the
Application command would invert the consumer-owned boundary.

## Decision

`P0.1-C4` production-composes exactly one internal `AcceptBuildPlan` command
on the existing CQRS bus. The existing handler receives:

- the existing production `IBuildPlanRepository` backed by migration `146`;
- the existing `RepositoryBuildPlanSourceRevisionPort` over the sole Sources
  `ISourceRevisionRepository`; and
- one `IdentityProjectsDeveloperWorkflowAuthorizationAdapter` implementing
  the consumer-owned `IDeveloperWorkflowAuthorizationPort`.

The authorization adapter is confined to Developer Workflows Infrastructure.
It resolves one active Identity Membership, loads active Resource Grants only
for a restricted membership, validates owner evidence, and delegates scope
semantics to Identity's sole `ResourceAccessEvaluator`. Only an admitted exact
scope may query the Projects owner for the exact Environment. Missing
membership, grant, or Environment remains concealed; repository and corrupted
owner-evidence failures remain typed and fail closed.

The Application handler continues to authorize before ACL parsing,
idempotency replay, and Sources resolution. It reparses the canonical proposal,
binds exact SourceRevision evidence, and commits only through the existing
BuildPlan repository transaction. No caller, controller, or adapter writes a
BuildPlan row or Outbox record directly.

C4 adds no public REST, client, CLI, or Management MCP surface; source checkout
or source-layout acquisition; table or migration; authorization evaluator;
SourceRevision; BuildRun; Workload; Execution; Route; Operation; queue; relay;
worker; retry rail; timer; scheduler; or downstream lifecycle write.

## Consequences

- Every production BuildPlan acceptance uses one authorization-first command,
  one Identity grant evaluator, one Projects Environment authority, one
  Sources evidence adapter, and one BuildPlan transaction.
- Developer Workflows Application remains expressed only in its own command,
  domain values, repository, and consumer-owned ports; foreign owner policy
  and persistence types stay in Infrastructure/composition.
- Architecture tests reject a second production construction, concrete
  persistence, delivery mechanism, or foreign policy import in Application.
- Trusted source-layout acquisition, public acceptance interfaces, profile
  acceptance composition, and downstream build/deployment handoffs remain
  explicit later P0 slices.
