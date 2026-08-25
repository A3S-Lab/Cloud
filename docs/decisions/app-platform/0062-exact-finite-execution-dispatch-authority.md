# 0062: Admit only the exact finite Execution profile

Status: Accepted

## Context

Workflow has one implemented Executions-owned application port for finite OCI
tasks. It resolves an exact immutable `ExecutionTemplate`, materializes bounded
JSON input, creates or adopts the ordinary Execution and Operation, and links
that child to the parent A3S Flow run.

The runtime-dispatch admission fence previously matched only the coarse
`executions` owner and `execution` kind. A caller-controlled descriptor could
therefore name `execution.code` or an arbitrary semantic profile and reach the
finite-task port even though those semantics and their provider evidence do not
exist. Descriptor admission metadata and a shared coarse kind are not proof
that two product capabilities have the same runtime contract.

## Decision

For descriptor-bearing revisions, the existing Executions workflow port admits
only a descriptor whose identity and semantic profile are both
`executions.finite`, whose owner is `executions`, whose coarse kind is
`execution`, and whose execution class is `owning_application_port`. The
existing capability validation continues to require an exact non-nil
`ExecutionTemplate` revision and digest with capability `execution.run`; Plan
validation continues to require one exact Environment.

`execution.code` remains unavailable until it has its own immutable input and
output contract, provider binding, cancellation and recovery behavior, and
retained evidence. The read-only node catalog remains discovery metadata and
cannot widen runtime admission.

Semantic-free historical compatibility is unchanged. Restoring immutable
revisions and replaying persisted Plans or Runs remains structural, while new
user-authored publication and new Goal, Plan, or Run compilation apply the
exact dispatch fence after authorized idempotency replay.

## Consequences

A caller cannot relabel the generic finite OCI task adapter as Code or another
Executions capability. Existing exact finite Execution graphs retain the same
runtime, Flow history, failure routing, default-output behavior, cancellation,
and recovery bytes.

This change adds no table, migration, API field, OpenAPI version, runtime
provider, scheduler, queue, or Flow schema. It does not make Code public or
complete `W0.4` or `W0.5`.
