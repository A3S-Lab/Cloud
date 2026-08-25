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

The contract contains no webhook signature, delivery body, provider
credential, checkout path, source revision, Environment mutation, BuildRun,
Workload, Route, Operation, cleanup worker, timer, or scheduler authority.
Committed pull-request fact dispatch, Preview state persistence, and owner
handoffs remain later P0.3 slices.
