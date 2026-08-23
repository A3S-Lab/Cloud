# P0.1 developer BuildPlan contract

`build-plan.acl` freezes the component-only `P0.1-C1` proposal emitted from an
exact source-layout snapshot. The canonical ACL binds the repository/commit
identity digest, accepted checkout content digest, detector revision, evidence
file, project root, and existing Sources-owned Dockerfile `BuildRecipe`.

The proposal is review evidence only. It does not accept a Source revision,
start a BuildRun, create a Workload, publish a Route, persist desired state, or
own a deployment scheduler. Later P0 slices must pass an explicitly accepted
plan through those existing owning contexts.
