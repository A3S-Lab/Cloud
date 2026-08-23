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

The C1 proposal remains review evidence only. C2 persists one immutable
acceptance per Source revision and project root through migration `146`, with
authorization-first internal CQRS, exact Sources evidence admission,
idempotency, audit, and Outbox. It does not accept a Source revision, start a
BuildRun, create a Workload, publish a Route, expose a public interface, or own
a deployment scheduler. Later P0 slices must pass the accepted plan through
those existing owning contexts.
