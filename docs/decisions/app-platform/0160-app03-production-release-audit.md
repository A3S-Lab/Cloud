# 0160. APP0.3 production-release audit (first principles)

- Status: Accepted
- Date: 2026-09-15
- Gate: APP0.3-C31

## Context

APP0.3-C1 through APP0.3-C30 are Implemented. Slice backlog for the delivery
role, publication route intents, Gateway rate shaping, claimed HTTP admit,
durable catalog, and retained/rollback ACL contracts is empty. The epic row
`APP0.3` and parity gate remain open: production release is not the same as
"all C-slices Implemented".

This audit freezes requirement-by-requirement evidence against the epic text
and parity capabilities. It refuses inventing path->channel, PublishRoute as
channel owner, Delivery rate middleware, automatic identity `request_headers`
emission, SSE/browser overfitting, REST catalog CRUD, and Redis distributed
limiter certification.

## Decision

Treat APP0.3 production release as **not achieved** until every epic clause
below is Proven or an explicit first-principles deferral is accepted by a later
gate. C1-C30 evidence does not close the epic.

### Requirement matrix

| Epic / parity clause | Verdict | Authoritative evidence | Gap |
| --- | --- | --- | --- |
| Bounded Delivery process role | **Proven** | ADR `0131`-`0133`; `ProcessRole::Delivery`; focused process-role tests | - |
| Delivery HTTP (anonymous + authenticated admit/lifecycle/observation) | **Proven** | ADR `0132`/`0134`-`0136`/`0138`; `/anonymous-delivery` + `/delivery`; OpenAPI; client/CLI | - |
| Delivery process drain | **Proven** | ADR `0137`; `DeliveryProcessDrain`; readiness `delivery-drain` tests | - |
| Identity Application grants + `application:invoke` tokens | **Proven** | ADR `0130`/`0133`; migration `209`; grant + scope tests | Secrets/Issue Environment material minting remains excluded (not required to invent) |
| Exact-release publication route intent (domain/persist/CQRS/REST/client) | **Proven** | ADR `0139`-`0144`; migration `210` | - |
| Gateway snapshot ACL load (planner + MCP + empty-site audit) | **Proven** | ADR `0145`-`0148` | PublishRoute is not channel owner |
| Bound rate-profile compile + runtime catalog + seeds + durable Postgres | **Proven** | ADR `0149`-`0152`/`0157`; migration `211` | REST catalog CRUD deferred; Redis limiter deferred |
| Exact-release binding table + claimed HTTP admit + reject metadata | **Proven** | ADR `0153`-`0155`; Gateway `application_publication` tests | Claim requires identity headers |
| Early Gateway apply of route `headers.request_headers` | **Proven** | ADR `0156`; `edge_request_headers_stamp_activates_admit` | Cloud `Edge::Route` has **no** `request_headers` field; stamps are Gateway ACL / traffic-owner headers, not automatic CP emission |
| Retained + managed rollback publications keep publication ACL | **Proven** | ADR `0158`/`0159` | - |
| Non-invented upstream identity stamp (intent/route binding) | **Accepted (claim-path)** | ADR `0161`: Delivery `/delivery` is traffic owner for API channels; Gateway stamps optional for claimed admit | Automatic path->channel / Edge emission still refused |
| `publication.api-blocking` / `api-streaming` / `embed` / `web` availability | **Accepted (scoped)** | ADR `0161`/`0162`: API channels `internal` via Delivery; embed/web `unavailable` by explicit deferral; gate `APP0.3` `implemented` | Public full-channel parity deferred |
| Shared SSE / cursors / browser-first UI | **Deferred (first principles)** | Explicit C1-C30 non-owners; refuse early SSE/browser overfitting | Not a C31 invent target |
| Failure recovery beyond Delivery drain + rollback ACL retain | **Accepted (defer invent)** | Drain + rollback ACL retain proven; ADR `0162` refuses inventing a second APP0.3 recovery rail; rely on existing APP0.2/H0 rails | No APP0.3-owned restart certification invented |

### Explicit non-owners (still refused)

- Inventing path->channel or automatic identity `request_headers` emission
- PublishRoute as publication channel owner
- Delivery-local rate-limit middleware
- REST Gateway rate-catalog CRUD
- Redis distributed limiter certification
- SSE/browser UI as the next forced slice

### Production-release rule

APP0.3 may move to production availability only when:

1. parity `gate "APP0.3"` evidence lists the Proven rows above with tests/ADRs, and
2. each APP0.3-gated publication capability that is claimed available has
   non-invented claim-path evidence (traffic owner or Gateway ACL stamps +
   Delivery), and
3. blocked identity stamp is either closed by a **non-invented** binding or
   explicitly accepted as operator/client-stamped claim-only with product docs.

Until C33: epic stayed open; C1-C32 remain Implemented component foundation.
ADR `0162` accepts scoped production release (internal API channels) with
explicit embed/web deferral and moves gate `APP0.3` to `implemented`.

## Consequences

- C31 is documentation + audit only: no runtime invention.
- Next development must pick a first-principles residual (non-invented claim
  path / product channel availability / accepted deferral), not invent stamps.
- After C33 / ADR `0162`, scoped production release (internal API channels +
  explicit embed/web deferral) is the accepted production meaning; full public
  publication parity remains a later gate.

## Evidence

- This ADR; plan row `APP0.3-C31`; architecture residual pointer; parity gate
  evidence refresh (state remains `planned`; capabilities remain `unavailable`)
- Spot-check: Gateway `cargo test --lib -- application_publication` (24 passed);
  Cloud `managed_rollback_retains_application_publication_route_intent_acl`
  (1 passed)
