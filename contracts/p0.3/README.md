# P0.3 pull-request Preview policy contract

`pull-request-preview-policy.acl` freezes the component-only `P0.3-C2`
configuration contract. Canonical
`a3s.cloud.pull-request-preview-policy.v1` binds one Developer Workflows-owned
policy to an exact active Sources subscription, GitHub installation,
repository, and base branch. It closes lifetime, active-count and resource
quotas, fork isolation, owner identity, and protected-Secret eligibility.

Acceptance is authorization-first and append-only. Migration `153` stores
continuous immutable policy revisions, idempotency references, audit records,
and Outbox events atomically. Owner and accepting-actor identities must be
members of the exact Organization. Every read reparses the canonical ACL and
checks its relational projections. Identical desired state converges without
another revision, regardless of the authorized caller.

`P0.3-C3` adds the Sources-owned committed-fact boundary used by this policy's
future consumer. After HMAC verification, the existing provider Inbox admits
one typed push or pull-request delivery. A new pull-request delivery fans out
one closed `source.pull-request-change.committed@1` fact per exact active
repository Subscription through the existing transactional Outbox. The stable
opaque change identity is bound to the Subscription, provider, and private
delivery identity. The Published Language contains only semantic repository,
branch, commit, pull-request, provider-time, and exact tenant binding; delivery
ID, signature, raw body, and raw-body digest remain Sources-private.

Migration `156` extends the single `source_webhook_inbox`; C3 creates no second
Inbox, Outbox, relay, retry rail, or worker. The contract contains no provider
credential, checkout path, source revision, Preview aggregate, Environment
mutation, BuildRun, Workload, Route, Operation, cleanup worker, timer, or
scheduler authority.

Component-only `P0.3-C4` production-composes one Developer Workflows projector
inside the existing Outbox Relay. Migration `157` persists one
policy-revision-bound Preview lifecycle row plus immutable consumer decision
receipts. New Previews select policy by the committed fact's event time;
subsequent facts retain that exact authority. Receipt replay, digest drift,
provider-time/content ordering, aggregate CAS, process restart, and atomic
state-plus-receipt persistence are closed without another Inbox, queue, retry
rail, or worker.

C4 still creates no Environment, BuildRun, Workload, Deployment, Route,
Operation, cleanup timer, scheduler, or public interface. Explicit
Projects/Artifacts/Workloads/Edge/Operations owner composition and handoff
remain later P0.3 slices, so pull-request Previews remain unavailable.
