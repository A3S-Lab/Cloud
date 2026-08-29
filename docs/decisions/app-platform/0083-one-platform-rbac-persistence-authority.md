# 0083: Serialize platform RBAC through one Identity persistence authority

Status: Accepted

## Context

The platform-role ACL and binding aggregates introduced in `C0.5-MT1-C1`
were not yet effective authority. A distributed control plane must prevent two
replicas from independently bootstrapping different roots, accepting competing
policy heads, removing the final recoverable owner, or replaying a mutation
under obsolete authority. A Redis lock, Gateway claim, cached role, or legacy
`actor_is_platform_admin` boolean cannot make those outcomes durable.

The Installation-scoped Audit and Outbox rail already exists. Adding a
platform-specific audit table, event queue, idempotency ledger, or lock service
would duplicate mechanism and create different commit boundaries for the same
Identity fact.

## Decision

`IPlatformRbacRepository` is the sole platform-policy and role-binding write
port. `PostgresIdentityRepository` implements it with one PostgreSQL
transaction per command. Every write locks the exact singleton
`cloud_installations` row before loading the current policy and role authority.
That row is the low-frequency Installation mutation serialization point shared
by every API replica; no cache or external distributed lock is truth.

Migration `177` persists:

- immutable accepted `platform_role_policy_revisions`;
- one `platform_role_policy_heads` row per Installation, which can advance only
  to the exact next revision of the same policy;
- versioned `platform_role_bindings`, with at most one active binding for an
  exact Installation and Principal; and
- deferred database recovery constraints that expose a policy head and its
  initial owner atomically, retain an active owner, reject disabling the final
  owner Principal, and make accepted or terminal history undeletable.

The first policy revision and first `platform_owner` binding bootstrap in one
transaction. Later policy acceptance uses an exact current-head compare-and-
swap. Binding creation, role change, and revocation use aggregate-version CAS.
The repository reloads the active actor and current policy under the same
Installation lock, evaluates closed `PlatformPermission` values, denies owner
administration by non-owners, denies self-escalation, and checks last-owner
recovery before mutation. Database triggers repeat the irreducible recovery and
transition constraints so direct SQL cannot bypass them.

Each successful transition writes through the existing global
`idempotency_records`, `audit_records`, and `outbox_events` mechanisms in that
same transaction. The facts carry exact Installation scope and a null
Organization. Policy ACL remains A3S ACL; dynamic event, audit, and idempotency
responses remain canonical JSON facts rather than a second configuration
format.

## Consequences

- Every replica observes one policy head, one active binding per Principal, and
  one recoverable owner invariant.
- A role cache may accelerate reads only after it is fenced by persisted
  revision/version evidence; revocation truth remains PostgreSQL.
- `C0.5-MT2-C1` establishes policy/binding persistence, not a public
  authorization surface. ADR 0085 now captures the active Principal, exact
  credential, current policy, and binding in one Identity decision transaction.
  Maintained concrete consumers and MT3 must still replace the legacy boolean
  bypass; the internal decision command is not a generic public evaluator.
- Tenant support remains separate. Its approval records must prove exact
  approver actions and liveness; caller-supplied approver IDs alone are not
  acceptance evidence.
- The retained PostgreSQL 17 gate exercises two repository instances racing on
  bootstrap, policy CAS, and owner revocation, then attempts direct database
  bypasses. It passed in [CI run 33220123607](https://github.com/A3S-Lab/Cloud/actions/runs/33220123607),
  [H0 job 99012267599](https://github.com/A3S-Lab/Cloud/actions/runs/33220123607/job/99012267599).
