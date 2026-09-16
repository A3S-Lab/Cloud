# 0242. Advertise Iteration as an internal Workflow node

- Status: Accepted
- Date: 2026-09-16
- Gate: W0.3-C1

## Context

`W0.3` already ships immutable composite-region admission, Flow-backed
bounded-parallel Iteration waves (ADR `0053` / Runtime v22), ordinal-stable
reduction, and focused domain/coordinator tests. The application-platform
parity manifest still classified `node.iteration` as `unavailable`, which
incorrectly described an implemented Workflow-local composite capability as an
absent implementation.

`internal` is deliberately distinct from public product availability. It means
an owning implementation slice exists and can be represented accurately by the
read-only node catalog; it does not verify remaining public Agent/business-
service availability, Loop/Answer claim close-out, `W0.4`/`W0.5`, or
`parity_claim`.

## Decision

1. **`node.iteration` is an internal, Workflow-owned node capability** with
   semantic profile `workflow.iteration`. Its evidence binds the existing
   composite-regions contract, ADR `0053` wave execution, domain wave
   authority, Flow coordinator, and focused parallel-iteration tests.

2. **Parity gate `W0.3` stays `in_progress`.** This slice does not claim Loop
   or Answer, does not mark `W0.3` `implemented`/`verified`, and does not set
   public `parity_claim`. Public Iteration availability remains closed.

3. **Still refused:** inventing a second orchestration authority, public
   product surface, graph editor, or treating this flip as full `W0.3`
   production foundation / public core parity.

## Consequences

- The catalog distinguishes the implemented Iteration foundation from nodes
  that still lack an admitted runtime claim path.
- `W0.3-C2`/`C3` may later claim Loop/Answer or close the production-
  foundation pattern without inventing BYOK/`S0` productization.
- Full production release still requires remaining in-progress gates and
  public `APP0.6` parity, not merely this internal advertisement.

## Evidence

- This ADR; plan row `W0.3-C1`; architecture + parity refresh
- Prior: ADR `0053`; `contracts/w0.3/composite-regions.acl`
- Implementation: `workflow_composite_wave.rs`,
  `coordinator/composite_wave.rs`
- Tests: focused parallel-iteration suites;
  `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`;
  `cargo test -p a3s-cloud-control-plane --lib workflow_node_catalog`

