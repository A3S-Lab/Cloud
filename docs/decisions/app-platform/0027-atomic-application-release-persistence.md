# 0027: Persist Application releases atomically

Status: Accepted

## Context

`APP0.1-C1` froze one Applications-owned immutable release contract for all six
application experiences. The next persistence slice must retain that single
authority while reusing Cloud's existing PostgreSQL, A3S ORM, idempotency,
audit, and transactional Outbox mechanisms. It must also prove that an
Application binds the exact admitted Workflow revision, not whichever revision
is current when the Application is read or invoked.

A persistence design that stores only a mutable Application head, copies the
Workflow graph, or commits release and event facts separately would weaken the
C1 authority and make replay or recovery ambiguous.

## Decision

Applications owns two PostgreSQL records:

- `applications` stores the project-scoped identity and sequence-fenced current
  release head; and
- `application_releases` stores the immutable canonical A3S ACL, digest-linked
  lineage, presentation evidence, and exact Workflow definition/revision plus
  contract, payload-set, semantic-contract-set, input-schema, and output-schema
  digests.

Migration `124` enforces immutable releases, sequential head advancement,
same-experience lineage, and a deferred exact current-release fence. Each
release references one exact project-scoped Workflow revision. Admission also
checks the stored Workflow content and payload-set digests, so a caller cannot
pair a real revision identity with foreign contract evidence.

The Applications repository locks the aggregate head for successor
publication. It commits the new release, head advance, shared idempotency
receipt, `application.release.published` audit record, and matching
transactional Outbox event in one A3S ORM transaction. Replay stores only the
exact organization/project/Application/release reference and reconstructs the
historic aggregate at that immutable release.

No session, message, Flow history, provider endpoint, Secret material, Gateway
route, Workflow graph, or provider execution state is stored by this slice.
Workflow and Flow retain graph and execution authority.

## Consequences

- A successful publication has one durable release, one matching head state,
  one idempotency result, one audit record, and one Outbox event, or none of
  them.
- Concurrent or stale publication cannot fork an Application release lineage.
- Idempotent replay remains stable after later releases advance the current
  head.
- Database reads reparse canonical ACL and reject drift between that contract
  and its indexed Workflow evidence.
- Authorization and public management or delivery surfaces remain separate
  `APP0.1`/later-gate work; persistence alone is not an availability claim.
