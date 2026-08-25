# 0057: Serialize exact Connector revision revocation with dispatch admission

Status: Accepted

## Context

A Workflow revision pins an immutable Connector profile revision and definition
digest. Secret-version revocation can prevent materialization, but it is not a
Connector lifecycle fact and cannot express that an otherwise valid Connector
revision must never start another provider attempt.

A read followed by a dispatch write would leave a race: revocation could commit
after the read but before the provider boundary. Mutating the immutable
`ConnectorRevision` would destroy historical replay authority, while cancelling
an already-dispatching provider side effect would claim a capability that the
HTTP provider contract does not expose.

## Decision

Connectors owns one immutable `ConnectorRevisionRevocation` fact for an exact
organization, project, environment, profile, revision number, revision ID, and
definition digest. The fact retains a bounded canonical operator reason, actor,
and timestamp. Migration `154` persists it with exact revision foreign keys,
idempotency, audit, and Outbox evidence. Resource authorization runs before
write replay or reads. REST/OpenAPI and the maintained TypeScript client expose
only an exact `POST`/`GET .../revisions/{revision_id}/revocation` boundary.

The PostgreSQL revocation transaction and `begin_dispatch` transaction both
lock the same exact `connector_revisions` row. Their commit order is therefore
the authority:

- dispatch intent first means that attempt remains `dispatching` and is later
  observed as in flight or indeterminate under the existing C6 rules;
- revocation first makes `begin_dispatch` reject before the provider call; and
- an already terminal attempt remains exactly replayable after revocation.

When a reserved attempt is rejected by this fence, the existing execution
service atomically settles body-free terminal `Rejected` evidence. This prevents
an unresolved reservation and lets the existing Flow Connector adapter consume
the ordinary terminal classification without a second retry or state machine.
The in-memory repository holds the corresponding revision-authority read lock
through its dispatch transition so deterministic tests preserve the same
linearization rule.

## Consequences

Cloud can stop future provider dispatch for one exact Connector revision while
preserving immutable revision, attempt, evidence, and Flow replay history. The
fact neither revokes Secrets nor cancels a provider side effect that has crossed
the dispatch fence. It adds no queue, scheduler, retry counter, provider state,
Flow history, plaintext material, or second HTTP client.

This closes the exact revision-revocation operation at component and management
API scope. It does not certify a real external provider, make HTTP Request
public, or complete `AUT0.5`, `W0.4`, or `W0.5`.
