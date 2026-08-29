# 0084: Persist one actual tenant-support approval authority

Status: Accepted

## Context

A support-grant ACL declares who must approve, but declared Principal IDs are
intent rather than proof that those humans actually approved. In a distributed
control plane, two replicas must not activate a grant from incomplete evidence,
obsolete policy, a disabled approver, a mismatched role binding, or a forged
row inserted outside the Application path. A Redis lock, caller-supplied list,
or a support-specific audit, event, or idempotency table cannot make approval
evidence durable.

Platform RBAC, the immutable Installation identity, and the shared scoped
Audit/Outbox/idempotency rail already provide the required authorities. Tenant
support therefore needs an Identity-owned transactional aggregate boundary,
not another approval engine or configuration format.

## Decision

`ITenantSupportGrantRepository` is the sole support-intent, actual-approval,
grant-activation, and terminal-revocation write port.
`PostgresIdentityRepository` implements every command in one PostgreSQL
transaction after locking the canonical Installation row and resolving the
current policy, active human Principal, and exact active role binding.

Migration `178` persists four related histories:

- immutable `tenant_support_grant_intents` with the canonical grant ACL and
  digest;
- immutable `tenant_support_grant_required_approvers` projected from that
  accepted intent;
- immutable `tenant_support_grant_approvals` binding each actual approver to
  exact authentication, current policy revision/digest, role-binding
  ID/version, contract digest, time, and evidence digest; and
- `tenant_support_grants`, activated only after every required actual approval
  exists and retaining one terminal revocation generation.

Proposal and revocation require current `platform:tenant-support:manage`
authority. Approval additionally requires the caller to be one declared,
active human approver whose current policy and role binding admit that
permission. The threshold-crossing transaction locks the intent and derives
`accepted_at` from the maximum persisted approval time. Database triggers
repeat the irreducible lineage, evidence, immutability, threshold, and terminal
transition rules so direct SQL cannot turn declarations into approval facts.

Every successful transition reuses `idempotency_records`, `audit_records`, and
`outbox_events` in the same transaction and exact Installation or tenant scope.
No support-specific lock, queue, cache, audit, event, or replay ledger is
introduced.

## Consequences

- Concurrent replicas can record distinct required approvals, but exactly the
  threshold-crossing transaction activates the grant.
- A disabled approver or obsolete policy/binding fails the complete
  transaction; a partial final approval cannot remain committed.
- Grant acceptance is evidence-derived, immutable, non-renewing, and terminal
  after revocation.
- This slice establishes persisted support authority, not a public privileged
  allow interface. The next slice must atomically snapshot the active
  Principal, verified credential, current policy/binding, and exact optional
  grant into the one privileged decision model.
- The retained PostgreSQL 17 gate races actual dual approvals across two
  repository instances and exercises forged, incomplete, disabled, replayed,
  and direct-SQL paths. It passed in [CI run
  33224399567](https://github.com/A3S-Lab/Cloud/actions/runs/33224399567),
  [H0 job
  99025035853](https://github.com/A3S-Lab/Cloud/actions/runs/33224399567/job/99025035853).
