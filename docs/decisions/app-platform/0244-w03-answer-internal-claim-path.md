# 0244. Advertise Answer as an internal Applications node

- Status: Accepted
- Date: 2026-09-16
- Gate: W0.3-C3

## Context

`node.answer` is Applications-owned and depends on `APP0.2`. Descriptor-bound
Answer dispatch, root-bound repeated frames, and Answer failure routing already
exist under ADRs `0040`/`0043`/`0046` with Workflow coordinator and Applications
composition tests. The parity capability still listed only plan-doc evidence and
`unavailable`, which understated a proven owning-application port.

`internal` does not mean public chat streaming or `parity_claim`. It means the
catalog may project Answer accurately as an implemented Applications-owned
node under gate `W0.3` while `APP0.2` remains its semantic dependency.

## Decision

Advertise `node.answer` as `internal` under gate `W0.3`, keep the gate
`in_progress` until the W0.3 production-foundation close-out, expand evidence to
the accepted Answer ADRs plus owning implementation/tests, and refuse public
Answer or inventing a Workflow-local Answer authority.

## Consequences

- All `W0.3`-gated node capabilities can be represented as `internal`.
- `parity_claim` remains `false` and `public_claim_gate` remains `APP0.6`.
- A follow-up foundation slice may move gate `W0.3` to `implemented` without
  claiming `verified` or public availability.

