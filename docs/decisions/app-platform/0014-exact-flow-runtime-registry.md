# 0014: Route Flow work through one exact runtime registry

Status: Accepted

## Context

Cloud composes several product-owned `FlowRuntime` implementations behind one
A3S Flow engine. Workflow replay includes an exact workflow name and version,
but step invocation carries only an exact step name. The previous router used
exact workflow matching while routing steps by reserved prefixes and sending
every unmatched step to Deployment.

That fallback made Deployment an accidental global owner. A misspelled, newly
introduced, or incompletely registered step could cross a bounded-context
boundary before its actual runtime rejected it. Prefixes also could not prove
that every replay-supported step had one and only one owner.

## Decision

Cloud constructs one immutable runtime registry before connecting A3S Flow.
The composition root binds every current and replay-supported workflow
name/version to a product runtime. Each runtime module supplies the complete
exact step-name set implemented by that same module; the global registry binds
each exact name to its owner.

The production constructor accepts the concrete runtime type for each owner
before erasing it behind `FlowRuntime`. Swapping two owner implementations is
therefore a compile-time error rather than a valid but incorrectly routed
registry.

Registry construction rejects empty identities, duplicate workflow
name/version pairs, and duplicate step names before the process serves work.
Runtime lookup is exact. Unknown workflow identities and step names return a
closed error without prefix inference or a default product runtime.

Historic identities required for deterministic replay remain explicit:
Deployment v1-v4, placement-group Deployment v1-v2, and WorkflowRun v1-v3.
Retired Build v1-v4 remain deliberately absent and are cancelled by the
existing startup retirement policy; only the Box-native Build v5 runtime is
registered.

The registry does not become another scheduler, workflow catalog, or product
configuration surface. A3S Flow still owns durable replay and scheduling;
Cloud's product runtimes still own step implementations; A3S ACL remains the
only product configuration language.

## Consequences

A new workflow version or step is incomplete until its owning runtime exports
the identity and startup composition admits it without collision. Renaming a
persisted step requires an explicit replay-compatible entry rather than a
prefix convention.

Focused tests enumerate every owner-provided production step, preserve all
supported historic workflow identities, reject unknown identities, and prove
that both workflow and step collisions fail registry construction. Adding a
prefix router, default runtime, or second dispatch registry requires a
superseding decision.
