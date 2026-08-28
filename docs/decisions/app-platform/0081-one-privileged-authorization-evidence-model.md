# 0081: Use one privileged authorization evidence model

Status: Accepted

## Context

Platform operators need installation authority, while exceptional support may
need a narrow tenant scope. Treating a platform role as tenant authority would
expose source, prompts, responses, files, Secrets, checkpoints, Cell state,
model credentials or runtime exec. Treating every authorization result as A3S
ACL would also confuse desired configuration with an immutable runtime fact and
would duplicate the existing canonical decision-reference mechanism.

Installation-aware audit and Outbox persistence cannot be added safely by
placing global facts under a sentinel or synthetic Organization. The current
tables and event envelopes require an Organization, while Cloud does not yet
persist one canonical installation identity.

## Decision

Identity owns one privileged-authorization family with two different kinds of
evidence:

- desired role and grant intent is canonical A3S ACL parsed and generated only
  by `a3s-acl`; and
- an issued allow decision is an immutable canonical-JSON fact bound by a
  SHA-256 digest and the existing `AuthorizationDecisionRef` representation.
  `DecisionEvidenceRef` is only a neutral type alias for that representation.

Canonical `cloud.identity.tenant-support-grant.v1` binds an exact intended
Principal, complete Organization/Project/Environment lineage, a closed support
permission set, bounded case reference, justification digest, approval IDs,
tenant-notification, independent security-alert and post-incident-review
policy, start and expiry. The aggregate adds acceptance/version
evidence and one terminal revocation generation; the effective decision requires
the subject to be an active human. The permission set includes no Secret,
prompt, response, payload or interactive-exec permission. Standard grants last
from five minutes through four hours. Break-glass grants last from one through
thirty minutes and require tenant notification, an independent security alert
and post-incident review. Grants are immutable and non-renewing; revocation is
terminal.

A privileged allow decision resolves the current accepted platform-role policy,
an active Principal and one or more active exact-installation role bindings.
Tenant support additionally requires the intersection of:

```text
platform:tenant-support:use
  AND exact active human Principal
  AND active non-revoked TenantSupportGrant
  AND requested descendant scope
  AND one closed support permission
```

A platform role alone authorizes installation operations. The only tenant-scope
exceptions are tenant-lifecycle administration and support-grant metadata
read/manage; neither grants access to tenant application data. Every issued
decision embeds the exact accepted policy revision and canonical ACL snapshot,
binding versions, Principal version and kind, authentication evidence reference,
requested action/scope/resource/request identity, decision time and, when used,
the complete support-grant ACL plus lifecycle generation. Historical validation
therefore does not depend on Redis, a cache or mutable current rows.

The next persistence slice must introduce one canonical installation identity
and a discriminated `ScopeContext` for Audit and Outbox. It must migrate
existing Organization facts without inventing global tenant IDs. PostgreSQL/CAS
remains revocation and policy-head truth; cache or locks may only accelerate a
decision.

## Consequences

- There is no boolean administrator bypass and no second support-specific
  evaluator, evidence reference, audit log or configuration language.
- Closed platform permissions and closed support permissions remain different
  vocabularies; their intersection is explicit in one decision.
- Approver existence, active-human status, separation of duties, current policy
  head and persistence concurrency are Application/persistence obligations;
  component constructors alone grant no production authority.
- `C0.5-MT1-C2` is component-only. Canonical installation identity,
  installation-aware Audit/Outbox, repositories, Application interfaces,
  last-owner/self-escalation controls, cross-surface enforcement and hostile
  multi-replica evidence remain required.
