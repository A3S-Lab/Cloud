# 0015: Version every deployed Flow runtime generation

Status: Accepted

## Context

A3S Flow persists a runtime-build identity before replay so a worker can reject
history whose executable code it does not contain. Cloud introduced
`a3s-cloud-workflows@1` with that fence, then retained the same value while
adding WorkflowRun, typed-variable, placement-group, Durable Cell publication,
object-namespace recovery, and exact runtime-dispatch code.

A workflow name/version selects one deterministic workflow contract. It does
not identify the complete deployed router, step implementations, linked
runtime code, or dependencies. Reusing one build identity across those changes
therefore made distinct executable generations indistinguishable and weakened
Flow's admission fence.

## Decision

The exact runtime registry and its linked replay code are deployed as
`a3s-cloud-workflows@3`. Every newly created Cloud Operation obtains that
identity from the configured `FlowEngine`; no product context, interface, or
caller may supply a different current build identity.

`a3s-cloud-workflows@1` and `a3s-cloud-workflows@2` are explicit
replay-compatible migration generations, not aliases for current work. The
`@3` generation introduces WorkflowRun runtime v3 composite-hook decisions;
the current binary retains the registered
historic workflow versions and exact step identities needed by those runs.
Unknown pinned generations fail closed through A3S Flow. Histories created
before build pinning remain admitted only as bounded migration debt, and Cloud
continues to create no new unpinned Operation run.

Any later change to deterministic workflow decisions, step code, exact runtime
registration, or a linked input required for replay must introduce a new
current generation. An older generation may remain in the compatibility set
only while the same binary still contains and tests its required replay code;
otherwise a compatible worker must drain it or an explicit administrative
retirement must resolve it.

The compatibility set in `infrastructure::flow` is the only Cloud build
manifest. It configures Flow's native `RuntimeBuildCompatibility`; Cloud does
not add another build router, scheduler, queue, or product-owned compatibility
registry. Flow readiness projects the current identity, the complete admitted
set, and whether unpinned migration remains enabled.

## Consequences

Rolling workers cannot mistake newly pinned work for the former `@1` or `@2`
code generations. Known `@1`, `@2`, and unpinned migration histories remain recoverable,
while an undeclared build cannot mutate history. Tests prove the current,
explicitly compatible, unpinned-migration, and unknown-build cases separately.

The unpinned switch plus the `@1` and `@2` entries are visible migration debt. They must be
removed after retained PostgreSQL evidence proves no active history requires
them; they are not permanent compatibility defaults.
