# 0246. AUT0.5 production foundation for claimable Connector execution

- Status: Accepted
- Date: 2026-09-16
- Gate: AUT0.5-C12

## Context

AUT0.5-C1 through AUT0.5-C11 already prove the Connectors-owned exact-revision
HTTP execution path: profile/revision persistence, Resource Grant-aware CQRS,
public-Internet egress authorizer, terminal evidence, durable dispatch fence,
Workflow exact-attempt port, bounded retry/fallback policy, immutable response
objects, and authorized response reads. Gate `AUT0.5` remains `in_progress`.

No application-platform capability is gated on `AUT0.5` itself. The Workflow
HTTP Request node is already advertised `internal` under gate `W0.4` with an
`AUT0.5` dependency (ADR `0054`). Remaining AUT0.5 dependents
(`knowledge.datasource-web`, `toolkit.moderation`) stay `unavailable` on foreign
gates and must not be invented here.

Leaving `AUT0.5` `in_progress` after the connector production foundation is
proven blocks the same close-out pattern used by W0.3 / APP0.6 / C0.3.

## Decision

1. Move parity gate `AUT0.5` from `in_progress` to `implemented`.
2. Do not invent Knowledge web-datasource or moderation under this close-out.
3. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.
4. Leave `W0.4` / `A0.5` / `S0` on their own claim paths.

## Consequences

- Dependents may treat AUT0.5 as an implemented owning gate without claiming
  public Connector product availability.
- W0.4 foundation and Agent (`A0.5`) remain separate release blockers.
- BYOK / residency / air-gap (`S0`) remains refused-until-non-invented.

