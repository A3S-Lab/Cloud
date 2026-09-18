# Kense OS partnership — control-plane boundary

| Field | Value |
| --- | --- |
| Status | **Canon** (partner boundary) |
| Scope | What A3S Cloud owns vs what a Kense deployment may own beside Cloud |
| Pair | Kense `apps/web/docs/console/CONTROL-PLANE-BOUNDARY.md` |
| Does not | Change product gates in `ROADMAP.md` or claim Kense availability |

Conflict: this document and Kense CONTROL-PLANE-BOUNDARY agree that **A3S Cloud
is the sole OS control plane**. Kense documents may refine directory and OEM
extensions; they must not redefine Cloud aggregate ownership named in
[architecture.md](architecture.md).

---

## 1. Ruling

| Role | Owner |
| --- | --- |
| OS control plane (desired state, grants, routes, workloads, flow, inference policy, audit/outbox) | **A3S Cloud** |
| Management UX (Console IA, dashboards) | Kense Console — not Cloud |
| People directory (Member, Department, HR-shaped User tree) | Kense extension API — maps into Cloud Principals/Grants via explicit links |
| Data-plane enforcement (expensive calls) | **A3S Gateway** driven by Edge snapshots compiled from Cloud |
| Runtime convergence | Workloads → Fleet → Box/Runtime |

Cloud ships **no** management Dashboard. Cloud **does not** proxy tenant request
bodies. Cloud **does not** treat a partner API as a second writable authority for
the same aggregates.

---

## 2. Planes (do not collapse)

```text
Management plane:  Console/CLI/MCP  →  Cloud API (authority)
                                   ↛  Gateway CRUD

Data plane:        Clients + Capability Key  →  Gateway  →  providers
                   (limit, route, meter)

Runtime plane:     Cloud Workloads/Fleet desired state  →  Box receipts
```

Gateway is the **data-plane** choke point, not the sole HTTP entry for
organization administration.

---

## 3. Cloud IN / partner OUT (summary)

**Cloud owns:** Identity (Principals, memberships, resource grants, credentials),
Projects/Environments, Operations/Flow, Agents/Assets/Executions, Workflow,
Inference control plane (routes, ACL projection, usage ledger), Workloads, Fleet,
Edge desired snapshots, Sources/Artifacts/Assets, Connectors/Plugins, Data/S0
contracts, Audit/Outbox/Notifications, management REST/CLI/MCP.

**Cloud does not own:** Console IA, OEM commercial ledgers, Token Hub/wallets,
vertical solution UIs, partner HR directory trees, browser-side authorization.

**Partner may own (extensions only):** Member accounts, Department directory,
install/ops config KV that is not product desired state, read-only model catalogs,
*transitional* blob pull/Git/OCI convenience faces that admit into Cloud by
digest — never a parallel Grant/Route/Workload store.

Deferred Cloud product lanes (FaaS, Durable Cell, WEB0, PW0/I0, …) remain Cloud
roadmaps; partners must not fork them into a second control plane.

---

## 4. Minimum federation contracts

| Contract | Meaning |
| --- | --- |
| SubjectLink | Partner user/member ↔ Cloud Principal via admin-managed `partner-*` keys on `external_identity_links`. Link/revoke/list-by-principal: admin + `identity:write`. Resolve: org member + `cloud:read` at `GET …/partner-subject-links/resolve`. Kense key `partner-kense-directory`; UUID subjects only. |
| DirectoryProjection | Department/group as grant *subject reference* plus membership projection. Cloud admits opaque `{issuer}#department/{uuid}` / `{issuer}#group/{uuid}` refs on `directory-resource-grants` and syncs Principal bindings on `directory-membership-projections` (admin + `identity:write`). Auth-time expansion includes active directory grants for subjects bound to the acting Principal; partner directory trees are not stored. |
| ArtifactAdmission | Partner-produced bytes enter Cloud only as digest-admitted artifacts. `POST/GET /organizations/{organizationId}/partner-artifact-admissions` records `sha256:` digest, kind (`model`/`git`/`oci`/`generic`), byte size, and opaque `partnerRef`. Cloud does not store partner blob bytes. |
| EntitlementCompile | Marketplace entitlement compiles inside Cloud; Gateway reads snapshots |

No shared writable tables across processes. On conflict, Cloud resource desired
state wins; directory content wins only for HR-shaped fields and never overrides
Grants.

---

## 5. Document links

- Stable Cloud target: [architecture.md](architecture.md)
- Product gates: [../ROADMAP.md](../ROADMAP.md)
- Kense pair canon: repository `kense` → `apps/web/docs/console/CONTROL-PLANE-BOUNDARY.md`
- Kense delivery waves: repository `kense` → `apps/web/docs/console/ROADMAP.md` (consumes Cloud gates; does not redefine Verified)
