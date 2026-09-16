# 0248. A0.5 production foundation for Skill bind path without external-provider verification

- Status: Accepted
- Date: 2026-09-16
- Gate: A0.5-C1

## Context

Parity gate `A0.5` is `in_progress` with only `doc:ROADMAP.md` evidence, while
the repository already contains the Skill bind vertical slice:

- migration `067_skill_workload_revision_bindings.sql`
- Workloads anti-corruption admission
  `skill_release_admission.rs`
- Real Box Skill lifecycle fixtures under
  `real_box_release/skill_lifecycle.rs`
- Assets catalog / Skill bundle media-type enforcement

ROADMAP still correctly blocks **verification** until retained
external-provider evidence lands. Leaving the gate `in_progress` with a single
roadmap citation understates the claimable foundation and blocks honest
production accounting. Closing to `verified` or inventing app-platform
capabilities owned by `A0.5` would overfit.

No app-platform capability currently uses `gate = "A0.5"`; dependents such as
`application.new-agent` remain under `APP0.4`.

## Decision

1. Move parity gate `A0.5` from `in_progress` to `implemented`.
2. Replace thin roadmap-only evidence with the Skill bind implementation,
   migration, focused tests, this ADR, and the plan row.
3. Do not invent new `A0.5`-gated capabilities.
4. Do not mark `A0.5` `verified` or raise `parity_claim` / public advertisement.
5. Keep `parity_claim=false` and `public_claim_gate=APP0.6`.

## Consequences

- A0.5 is an honest production foundation for Skill archive bind/rebind/
  unbind on the existing Assets ??Workloads path.
- External-provider / real forge verification remains an explicit later gate
  promotion, not this close-out.
- APP0.6 public claim stays blocked until every advertised public capability
  sits on verified owning gates.

