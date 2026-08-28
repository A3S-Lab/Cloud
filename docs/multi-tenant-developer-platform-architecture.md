# A3S Cloud Multi-Tenant Developer Platform Architecture

## 1. Decision and current boundary

A3S Cloud is a **multi-tenant AI developer platform**. Multi-tenancy is not a
presentation filter or an optional hosted-service profile; it is a cross-cutting
invariant of every control-plane aggregate, supply path, execution, route,
object, credential, quota, usage fact, and cleanup decision.

The canonical tenant hierarchy is:

```text
A3S installation
  -> Organization (security, ownership and commercial-attribution boundary)
     -> Project (team and product collaboration boundary)
        -> Environment (deployment, Secret, route, quota and policy boundary)
```

`Workspace` remains a context-qualified execution or A3S Use concept. It does
not become a fourth general tenancy root. Agent workspaces, Use workspaces and
source working trees bind an exact Organization/Project/Environment scope and
retain their owning context's lifecycle.

The current code already has Organization, Principal, Membership,
MembershipInvitation, API credential, Project, Environment and Resource Grant
foundations, including exact Project/Environment/Node scopes. That is not yet a
complete production multi-tenant platform. Installation-level human RBAC,
cross-context scope ratchets, capacity fairness, privileged support access and
the complete isolation matrix remain gated below and must not be advertised as
available early.

Component-only `C0.5-MT1-C1` now freezes one explicit `ScopeContext` with the
exact Installation/Organization/Project/Environment lineage, canonical
`cloud.identity.platform-role-policy.v1`, deterministic accepted policy
revisions, closed roles and permissions, and the `PlatformRoleBinding` domain
lifecycle. This adds no repository, effective authorization decision,
installation-scoped audit/Outbox persistence or public interface. The legacy
`actor_is_platform_admin` boolean remains migration debt and is not valid proof
for new MT1 or workload-identity paths.

## 2. One identity authority, two authorization planes

Identity is the sole Principal, credential, role-binding, grant, federation,
session and revocation authority. Platform administration and tenant
collaboration are different scopes inside that authority; they are not two
identity systems.

### 2.1 Installation administration plane

System administrators operate installation-wide resources that do not belong
to an arbitrary tenant: system process revisions, cluster/node pools, shared
capacity policy, global Provider certification, Registry trust, platform
limits, upgrades, backup/restore, disaster recovery and global audit/security
operations.

The target Identity model adds an installation-scoped `PlatformRoleBinding`
for an exact active human or service Principal. It uses a closed initial role
set and closed permissions:

| Platform role | Intended permissions | Explicit exclusions |
| --- | --- | --- |
| `platform_owner` | Bootstrap recovery, platform-role administration and every administrator permission; at least one recoverable owner must remain | No implicit tenant Secret, payload or application-data access |
| `platform_admin` | Node-pool/capacity policy, Provider and Registry trust, platform configuration, upgrade, backup/restore and tenant lifecycle administration | Cannot grant or remove `platform_owner`; no implicit tenant data access |
| `platform_operator` | Observe and operate nodes, system roles, deployments, incidents, drains and bounded recovery actions | Cannot change identity roots, Registry trust, Provider credentials or retention policy |
| `security_auditor` | Read redacted platform audit, configuration evidence, security findings and signed exports | No mutation, Secret material, tenant payload or runtime exec |

Roles are convenience bundles. Authorization evaluates closed permission IDs,
not string comparisons scattered through controllers. A versioned role-policy
revision maps roles to permissions, Identity compiles the effective decision,
and every Application handler asks that one decision port before loading or
mutating protected state.

Interactive tenant credentials never receive `platform:*` permission merely
because their Principal owns an Organization. Bootstrap/service credentials
never become human sessions. Granting, changing or revoking a platform role is
version-checked, idempotent, strongly audited and protected against self-
escalation, last-owner removal and replay.

### 2.2 Tenant collaboration plane

Organization Membership remains the tenant relationship. Its closed roles are
`owner`, `admin`, `member`, and `restricted`; a restricted Membership receives
only explicit `ResourceGrant` scopes. Project grants cover their descendant
Environments, Environment grants are exact, and a lower scope can never expand
an upper scope.

Every tenant action requires the intersection of:

```text
active Principal
  AND active non-revoked credential/session
  AND credential action scope
  AND active Organization Membership
  AND role permission
  AND applicable Resource Grant, when restricted
  AND owning-context resource policy
  AND current session / network / risk constraints
```

Authorization precedes idempotency replay and resource lookup so guessed IDs,
counts, timings, cursors and replay keys do not disclose another tenant.
Queries are filtered by the same backend decision; a browser, CLI or MCP client
never receives a broad dataset to filter locally.

### 2.3 Privileged tenant support access

A platform role does **not** imply access to tenant source, prompts, responses,
files, Secrets, checkpoints, Cell state or model credentials. When operational
support genuinely requires tenant scope, Identity issues a separate bounded
`TenantSupportGrant` that pins:

- exact Principal, Organization and optionally Project/Environment;
- closed read or recovery permissions, never an unbounded administrator flag;
- incident/change-ticket reference and human-readable justification digest;
- approver, start, maximum expiry and revocation generation; and
- whether tenant notification or dual approval is required by policy.

Support grants are short-lived, non-renewing in place and prominent in audit.
Secret plaintext, prompt/response content and interactive Runtime exec remain
separately denied unless an even narrower capability has an explicit owning
gate. Emergency break-glass uses the same model with a shorter expiry,
mandatory reason, independent alert and post-incident review.

## 3. Tenant scope is part of identity

An aggregate ID alone is never a tenant boundary. Tenant-owned records use
scope-qualified identity and composite references:

```text
(organization_id, project_id?, environment_id?, aggregate_id, revision)
```

- Organization is mandatory for every tenant aggregate, Outbox fact, audit
  record, idempotency entry, object reference and usage record.
- Project is mandatory for team/product assets and descendants.
- Environment is mandatory for deployable intent, Secrets, external
  connections, Workloads, Routes, runtime bindings and live usage.
- Cross-context references repeat and validate the applicable scope. A UUID
  collision or guessed foreign UUID cannot create a relationship.
- Every immutable revision preserves original attribution even when names,
  memberships, cost centers or policies later change.

Global platform records use an explicit installation scope; they never use a
synthetic Organization. Shared catalog templates are read-only candidates.
Adopting one creates or binds an exact tenant revision under the owning
context.

## 4. Multi-tenant developer supply and execution

| Capability | Isolation contract |
| --- | --- |
| Hosted Git | Repository identity includes Organization/Asset; authorization is rechecked for Smart HTTP; refs/objects never infer tenant from a path alone |
| External sources | Provider connection and exact SourceRevision are tenant-bound; short-lived provider credentials cannot cross Organization/Environment |
| Builds | BuildRun binds exact tenant source, policy and output; Runtime Task/Box receives only scoped materialization grants and bounded egress |
| OCI Registry | Repository namespace and pull/push credential are tenant-scoped while manifest digest stays immutable; Artifacts owns accepted publication evidence |
| A3S Use Registry | Registry trust may be platform- or Organization-approved; package assignment and workspace impact are exact tenant intent |
| Models and weights | Catalog visibility, license acceptance, import Secret, object namespace, cache entitlement and deployment binding preserve Organization/Environment scope |
| Agent / Workflow / Function / MCP / Application | Release and invocation identities bind the tenant hierarchy; cross-product calls pass typed IDs and owner ports, never ambient tenant context |
| Durable Cell | Cell application and public namespace bind the tenant; the provider cannot use a caller-supplied tenant header to select state |
| Static Web | Web manifest, object namespace, Application binding, domain and Gateway cache key include tenant/release identity |
| Runtime/Box | UnitSpec carries opaque scope and policy digests plus scoped Claims/Secrets; Box isolation prevents filesystem, process, network, device and workspace crossover |
| Gateway | Route/key/session resolves one immutable tenant scope before policy; cache, limits, affinity, usage and logs are scope-qualified and non-enumerating |

Logical S3 object references are tenant/namespace/digest bound. Provider bucket
names and credentials stay behind the one object authority. Physical
co-location is allowed only when authorization, namespace construction,
encryption, retention, list prohibition and deletion tests prove equivalent
isolation.

## 5. Shared CPU/GPU capacity without noisy neighbors

Workloads and Fleet remain the sole placement and Claim authority for every
tenant. Multi-tenancy adds hierarchical admission, not another scheduler.

Quota is reserved before work at the narrowest effective limit and charged to
all ancestors:

```text
installation capacity
  -> Organization allocation
     -> Project allocation
        -> Environment policy
           -> Workload / Task / request reservation
```

Typed dimensions include CPU, memory, PIDs, ephemeral/local cache bytes,
persistent/object bytes, build concurrency, Task concurrency, Service replicas,
public routes/domains, outbound bandwidth, GPU device/partition and time,
loaded model bytes, inference request/token concurrency, Agent sessions,
Workflow runs, Function activations and Cell connections/storage.

The Workloads admission transaction reserves durable capacity or rejects with
a stable reason before node dispatch. The sole autoscaler cannot exceed the
same effective quota. Fair queues use bounded per-Organization and per-Project
weights, starvation limits and priority classes; a tenant cannot create a
private side queue through Agent, Workflow, FaaS or inference semantics.

Physical Nodes and shared pools are Fleet platform resources. A separately
gated bring-your-own pool may bind an Organization capacity owner and placement
eligibility, but it remains one Fleet Node/Claim model and stays visible to
system administrators. A tenant admin cannot mutate another tenant's
allocation or platform pool policy.

## 6. Network, data and request isolation

- Nodes initiate the Fleet control channel; tenant workloads cannot call the
  Cloud database, NATS, node-agent control endpoint or Box control socket.
- Workload egress is denied or allowlisted by immutable environment policy.
  DNS resolution and exact endpoint authorization occur immediately before
  each external attempt.
- Private service endpoints enter only exact generation-bound Gateway target
  snapshots. Tenants never receive Runtime/Box addresses.
- Secrets materialize just in time to an exact attempt/unit/destination and are
  absent from ACL, PostgreSQL facts, events, snapshots, logs and audit.
- PostgreSQL repository queries and mutations include tenant predicates;
  composite keys/foreign keys and architecture tests prevent foreign-scope
  joins. Process-specific database roles limit blast radius.
- Search, logs, metrics, usage, audit and diagnostics use authorized projections
  with bounded label cardinality. Prompts, responses and Secret values are not
  management telemetry.
- Object and cache deletion is fence- and reference-aware. Organization
  deletion cannot erase bytes still retained by a legal hold or another valid
  immutable reference, and it cannot leave tenant-addressable residue.

## 7. Tenant lifecycle

Organization, Project and Environment lifecycle is explicit and asynchronous:

1. create scope and its initial authorized ownership;
2. configure identity federation, policy, quotas, supply trust and attribution;
3. admit immutable product intent and deploy only within effective policy;
4. suspend new mutations and request traffic without destroying evidence;
5. export authorized metadata/objects and create a signed inventory;
6. drain/cancel workloads and revoke credentials/routes/provider access;
7. apply retention, legal-hold and deletion policy through owning contexts; and
8. close only after a zero-addressability and bounded-residue report.

No context implements its own Organization deletion workflow. Projects owns
scope lifecycle intent; Operations/Flow coordinates owner ports; every context
returns typed completion/blocking evidence; Audit retains the required final
record.

## 8. Management surfaces

A3S Cloud ships no management Dashboard. Multi-tenant and system-administrator
capabilities are exposed through the same REST/OpenAPI, maintained client, CLI
and applicable Management MCP Application contracts.

Management MCP intentionally excludes operations whose arguments would expose
Secret plaintext, identity proofs, break-glass tokens or unrestricted runtime
input to a model. CLI reads private values from bounded stdin, never argv.
Every surface must produce the same allow/deny outcome and the same audit fact
for an equivalent Principal, credential and command.

`WEB0` hosts tenant Agent/Application UIs only. Such a UI receives no hidden
administrator API, and its user session is evaluated exactly like any other
client.

## 9. Delivery gates

The work extends `C0.3`, `C0.5`, `H0.5` and every product gate; it does not
create a second tenancy milestone or authorization engine.

| Gate | Required outcome |
| --- | --- |
| `C0.5-MT1` | Freeze installation versus Organization/Project/Environment scope, closed platform permissions, `PlatformRoleBinding`, `TenantSupportGrant`, effective-decision and audit ACLs |
| `C0.5-MT2` | Persist role bindings/support grants with last-owner, self-escalation, expiry, revocation, idempotency and immutable-history invariants; migrate no platform record through a synthetic tenant |
| `C0.5-MT3` | Enforce system-admin, organization-owner/admin, member/restricted and service-Principal matrices through one Identity decision port across REST/client/CLI/Management MCP |
| `C0.5-MT4` | Ratchet every tenant aggregate, repository, composite foreign key, Outbox/audit/idempotency/object reference and cursor to exact scope; zero presentation-only or browser-side tenant filtering |
| `H0.5-MT5` | Prove hierarchical CPU/GPU/storage/request quotas, fair admission, autoscaling caps and noisy-neighbor bounds under concurrent Organizations without another queue or scheduler |
| `C0.5-MT6` | Prove OIDC/SAML/SCIM, session/MFA policy, platform-role provisioning restrictions, tenant suspension/export/deletion and time-bounded audited support access |
| `C0.5-MT7` | Run adversarial cross-tenant ID/cursor/replay/cache/object/Secret/network/route/log/search/usage tests plus system-admin privilege-escalation and break-glass recovery on real PostgreSQL/S3/Gateway/Runtime/Box providers |

`C0.5-MT1-C1` is implemented only as a domain component. `MT1` remains open
for `TenantSupportGrant`, effective-decision and audit ACLs, plus an explicit
installation-scoped audit/Outbox contract. `MT2` then persists those contracts
and their concurrency invariants; `MT3` replaces boolean administrator bypasses
with the one Identity decision port. No platform-RBAC availability is claimed
before those gates and the later adversarial evidence pass.

Production multi-tenancy is not complete until every product lane also proves
its own scope-specific failure and cleanup cases. Passing Identity unit tests
alone cannot certify it.

## 10. Non-goals

- A second system-admin user directory, RBAC evaluator or audit store.
- Treating an Organization owner as a platform administrator.
- Giving a platform administrator implicit tenant data or Secret access.
- Encoding roles in API keys, route names, node labels or UI modes.
- A database/schema/control plane per tenant as the default isolation model.
- Browser-side filtering, ambient thread-local tenant context or unscoped UUID
  repository methods.
- Per-product quotas, schedulers, credential issuers or tenant deletion flows.
- A Cloud management Dashboard or UI-specific authorization backend.
