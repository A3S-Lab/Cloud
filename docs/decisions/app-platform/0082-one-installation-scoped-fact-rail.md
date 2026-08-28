# 0082: Use one Installation-scoped fact rail

Status: Accepted

## Context

A3S Cloud owns both installation resources and tenant resources. The existing
Audit and Outbox tables required an Organization, so a platform policy,
system-administrator decision, trust root, migration or recovery fact could not
be represented truthfully. Assigning those facts to a sentinel Organization
would make global authority look tenant-owned and weaken authorization,
retention, foreign-key and incident evidence. Creating separate platform Audit
or Outbox tables would duplicate relay, retry, observability, retention and
recovery mechanisms.

An uncommitted domain fact also cannot safely accept an Installation ID from a
tenant caller. The database must resolve the canonical owning Installation from
the exact Organization/Project/Environment lineage before the fact is durable.
The migration must remain compatible with already-running Organization-only
writers during a bounded rolling upgrade.

## Decision

Each independently deployed Cloud control plane persists exactly one immutable
`cloud_installations` row. A checked singleton key prevents a second row; an
immutability trigger prevents identity update or deletion. PostgreSQL creates
the UUID, and every Organization receives that canonical owner through a
database default and foreign key. Public callers never choose tenant ownership.

One closed `CloudScopeRef` describes an uncommitted fact:

- Installation carries the exact Installation ID;
- Organization carries the Organization ID;
- Project carries Organization and Project IDs; and
- Environment carries Organization, Project and Environment IDs.

Tenant references intentionally omit Installation. The shared PostgreSQL write
boundary validates the reference, locks the exact canonical owner rows with
`FOR SHARE`, resolves their persisted Installation, and constructs the full
`ScopeContext`. Audit attribution is derived from that resolved scope; payloads,
headers, caches and ambient request state are never ownership sources.

Migration `174` evolves the existing `outbox_events` and `audit_records` tables
in place. Both store one closed scope discriminator and complete nullable
lineage. Installation facts require null tenant columns. Tenant facts require
their exact ancestors and foreign keys, and scope is immutable after insert.
Existing rows are backfilled as exact tenant facts. Database defaults map old
Organization-only writers to Organization scope during the bounded mixed-version
window; every new writer persists scope explicitly.

The existing transactional Outbox relay and A3S Event publisher remain the only
integration-fact mechanism. The existing Audit table remains the only audit
authority. Installation audit is retained indefinitely in this foundation;
tenant retention continues through the existing Organization authority. A3S
Event carries the full scope and a derived nullable legacy `organizationId` for
bounded consumer migration, never a second stored ownership value.

## Consequences

- Platform facts no longer require a synthetic or sentinel Organization.
- Installation and tenant facts share one transaction, relay, retry,
  observability and recovery path.
- Every consumer must handle a fact with no Organization and fail closed when
  tenant behavior receives Installation scope.
- The migration deliberately supports old Organization writers, but only new
  code may create Installation, Project or Environment fact scope.
- `C0.5-MT1-C3` establishes persistence identity and fact scope only. MT2 still
  owns platform-policy/binding/grant repositories, current-head, approval,
  last-owner, self-escalation, idempotency and concurrency rules; MT3 still owns
  cross-surface enforcement and removal of boolean administrator bypasses.
- Hosting more than one logical Installation in one database would require a
  superseding decision and migration; callers cannot opt into it by supplying
  another UUID.
