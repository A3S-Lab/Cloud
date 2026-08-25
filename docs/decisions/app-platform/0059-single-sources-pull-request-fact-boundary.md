# 0059: Publish pull-request changes through the single Sources delivery boundary

Status: Accepted

## Context

The GitHub adapter already authenticated pull-request lifecycle payloads, while
Developer Workflows already reduced a minimal semantic observation into one
deterministic Preview decision. Passing the verifier DTO directly across the
bounded-context boundary would expose delivery evidence and provider parsing
semantics. Adding a pull-request table, queue, relay, or retry worker would
duplicate Sources' existing authenticated Inbox and the platform's
transactional Outbox mechanism.

One provider delivery can also match more than one active repository
Subscription. A consumer needs the exact Organization, Project, Environment,
and Subscription binding that was authoritative when Sources committed the
observation; it must not query mutable Sources state later to reconstruct that
fanout.

## Decision

Sources owns one polymorphic `SourceWebhookDelivery` envelope with a closed
push-or-pull-request payload. Migration `156` extends the existing
`source_webhook_inbox` with a discriminator and exact pull-request evidence.
The `(provider, delivery_id)` key remains the sole provider-delivery
deduplication authority. Reusing that key with different semantic content or a
different raw-payload digest fails with conflict.

For a newly committed pull-request delivery, the same Inbox transaction locks
the authoritative active GitHub connection and exact active repository
Subscriptions. It creates one immutable
`source.pull-request-change.committed@1` Published Language fact per matching
Subscription and writes every envelope through the existing Outbox. Any fact
or Outbox failure rolls back the Inbox insert and the complete fanout. Replay of
the same committed delivery emits nothing again.

The fact uses a deterministic opaque `SourcePullRequestChangeId` derived from
the Subscription, provider, and private provider-delivery identity. Its public
payload carries only exact tenant and Subscription binding, installation, base
and head repository/branch, head commit, pull-request identity, closed change
kind, merge state, and provider timestamps. Provider delivery ID, signature,
raw payload, and raw-payload digest stay inside Sources.

Push deliveries retain their existing SourceRevision path. Pull-request
deliveries create no SourceRevision and do not reserve the push-only
revision-delivery mechanism. Sources creates no Preview, Environment, BuildRun,
Workload, Route, Operation, timer, or scheduler state.

## Consequences

Consumers can reduce a committed, exact-Subscription-bound fact without
importing Sources aggregates, repositories, verifier types, or private evidence.
Sources remains the sole owner of provider authentication and delivery
deduplication, while the shared Outbox/Relay remains the sole publication
mechanism.

This decision certifies only the Sources producer boundary. Developer Workflows
still needs an idempotent consumer/projection, persisted Preview lifecycle, and
explicit Projects, Artifacts, Workloads, Edge, and Operations owner handoffs
before pull-request Previews are available.

Follow-up decision [0061](0061-single-developer-preview-projection-authority.md)
implements the consumer projection and persisted lifecycle while leaving every
resource-owner handoff and availability gate open.
