# 0080: Keep installation scope and platform RBAC in one Identity authority

Status: Accepted

## Context

A3S Cloud has tenant-owned resources under Organization, Project and
Environment, but it also has installation-owned resources such as trust roots,
node pools, shared capacity policy, provider certification, Registry trust,
upgrades, backup/restore and disaster recovery. Mapping those installation
records to a synthetic Organization would weaken ownership, foreign-key,
audit, retention and recovery semantics.

The existing `actor_is_platform_admin` boolean is a transitional presentation
fact, not a durable authorization decision. Carrying that boolean into new
workload-trust or multi-tenant persistence would permit callers and replicas to
disagree about authority and would create a second mechanism beside Identity's
Membership and Resource Grant decisions.

## Decision

One explicit `ScopeContext` represents the complete Cloud authority hierarchy:

- Installation;
- Installation plus Organization;
- Installation plus Organization and Project; or
- Installation plus Organization, Project and Environment.

Every narrower scope repeats its complete parent lineage. Intersection returns
only an existing narrower operand when the operands are ancestor-related; it
never manufactures authority. The value is passed explicitly across
Application and owner-port boundaries. It is not inferred from headers,
thread-local state, a Workspace, or an unqualified child UUID. Installation
records never use a synthetic Organization.

Identity remains the sole role and grant authority. It owns canonical
`cloud.identity.platform-role-policy.v1` A3S ACL, its accepted immutable
revisions and installation-scoped `PlatformRoleBinding` lifecycle. The closed
roles are `platform_owner`, `platform_admin`, `platform_operator`, and
`security_auditor`; authorization uses closed `platform:*` permission IDs,
never controller-local role-name checks. Immutable role ceilings prevent a
policy revision from widening a role, while the owner retains every closed
platform permission needed for recovery.

A binding names a role rather than copying permissions or pinning an obsolete
policy revision. An effective decision must resolve the current accepted
policy, active Principal and active binding together, and must record the exact
policy revision and digest in audit evidence. Revocation therefore takes
effect on the next decision without rewriting every binding.

Platform roles authorize installation operations only. They never imply access
to tenant source, prompts, responses, files, Secrets, checkpoints, Cell state,
model credentials or runtime exec. Such access requires a separate bounded
`TenantSupportGrant` under the tenant authorization plane.

Policy and binding writes will use optimistic concurrency, idempotency,
last-owner protection, self-escalation denial and atomic installation-scoped
Outbox/audit evidence. Until those application and persistence slices exist,
the current component contracts grant no effective authority. New MT1 and WI1
work may not accept `actor_is_platform_admin` as proof.

## Consequences

- There is one Identity authority with distinct installation and tenant
  authorization planes, not a system-admin directory beside tenant identity.
- Shared scope carries identity and narrowing semantics only; it owns no
  tenant, project, deployment, audit or runtime lifecycle.
- Roles remain bounded convenience bundles. Permissions and the requested
  scope are the stable decision vocabulary exposed to consuming contexts.
- Redis, caches, Gateway claims and distributed locks may accelerate a current
  decision but cannot become role-policy, binding or revocation truth.
- `C0.5-MT1-C1` is component-only. ADR 0081 defines the following support-grant
  and effective-decision evidence slice. Installation-scoped Outbox/audit
  persistence, repositories, authorization interfaces, migration of boolean
  administrator bypasses and adversarial multi-replica evidence remain
  required.
