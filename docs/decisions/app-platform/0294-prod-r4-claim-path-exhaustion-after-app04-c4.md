# 0294. Cloud claim-path exhaustion after APP0.4-C4

- Status: Accepted
- Date: 2026-09-16
- Gate: production-release-honesty / PROD-R4

## Context

ADR `0283` treated claimable Nest attribute-macro hybrids as exhausted and kept
seven deferred/protocol leftovers imperative. ADR `0292` then treated Cloud-owned
`K0.2` file/text sealed contracts as exhausted at C1–C7.

ADR `0293` / plan row `APP0.4-C4` then claimed the last understated
Workflow-owned APP0.4 toolkit: `toolkit.variable-inspection` became `internal`
on the proven Flow-derived WorkflowRun variables claim path
(`GET .../workflow-runs/{workflow_run_id}/variables`, ADR `0010`). That slice
was allowed by ADR `0283` Consequences (“next claimable work must show
domain/application/infrastructure proof for a specific capability ID”) and did
not invent Nest wrappers around deferred leftovers.

A HEAD audit after `APP0.4-C4` shows no further Cloud claim-path candidates that
can change capability availability without inventing foreign owners:

| Remaining unavailable surface | Why Cloud alone cannot claim it |
| --- | --- |
| APP0.4 toolkits (moderation, snippets, node-test, error-policy, TTS/STT, templates, MCP façade, …) | No Applications/Workflow Nest claim-path owners; ADR `0167` deferral stands |
| `publication.embed` / `publication.web` | Explicit APP0.3 deferral (ADR `0162`); browser/generated-web product missing |
| `node.variable-assigner` | W0.4 foreign Applications path beyond W0.3 locals (ADR `0247`); compiler/semantic ports only, no presentation claim-path |
| W0.4 agent/code/knowledge/inference/tool nodes | Foreign module owners |
| `K0.2` / `K0.3` / `K0.5` `knowledge.*` | Needs workers + Sources/Use/Box/Runtime (ADR `0292`) |
| `U0.4` `plugin.*` | Executable plugin surfaces; assignment Nest is not `U0.4` |
| `I0.6` | OpenAI/media/speech productization beyond `I0.2` |
| `S0` / `MCP0.5` / `AUT0.4` integration-trigger | Unchanged refusals (ADR `0251` / `0283`) |

Public APP0.6 remains blocked: `parity_claim=false`, every capability stays
non-`public`, and gate `APP0.6` stays `implemented` (not `verified`).

## Decision

1. Treat Cloud Nest / claim-path availability flips as **exhausted** after
   `APP0.4-C4` for production-foundation purposes.
2. Keep the seven Nest deferred/protocol leftovers imperative (ADR `0283`).
3. Keep `I0.6`, `S0`, `K0.2`, `K0.3`, `K0.5`, `U0.4`, and `MCP0.5` `planned`
   until owning implementation evidence exists.
4. Refuse inventing availability for deferred APP0.4 toolkits, embed/web
   publication, `node.variable-assigner`, Knowledge product caps, executable
   plugins, BYOK/air-gap, integration-trigger without `U0.4`, or MCP0.5 joint
   closure from Nest alone.
5. Do not set `parity_claim=true`, do not mark `APP0.6` `verified`, and do not
   advertise remaining unavailable capabilities as `internal` without a new
   owning claim-path ADR.
6. Next production work must start a foreign-backed or worker-backed slice for a
   specific planned gate/capability ID (for example Knowledge file/text
   ingestion execution consuming C1–C7 digests), not further opaque claim-path
   audits.

## Consequences

- Production release is blocked on honest foreign owners and real workers, not
  on further Cloud Nest presentation or understated APP claim-path discovery.
- ADR `0283` / `0292` / `0293` remain the Nest, Knowledge sealed-contract, and
  variable-inspection baselines; this ADR is the post-`APP0.4-C4` claim-path
  exhaustion audit.

## Evidence

- This ADR
- ADR `0293` / plan row `APP0.4-C4`
- HEAD inventory of unavailable capabilities in
  `contracts/app-platform/v1/parity-manifest.acl`
- Focused `cargo test -p a3s-cloud-contracts --test app_platform_parity_manifest`
