# AI Application Platform Contracts

This directory contains the immutable, machine-checkable capability baseline
for the A3S Cloud AI application platform.

`v1/parity-manifest.acl` records the public reference baseline observed on
2026-08-13. It freezes 91 required outcomes across six application modes,
22 authoring/toolkit outcomes, 23 built-in node labels, six plugin outcomes,
13 Knowledge Pipeline outcomes, six publication channels, seven monitoring
outcomes, and eight enterprise outcomes.

Eight immutable `reference` entries pin the exact public source URLs and the
common observation date. Every capability cites at least one of those source
identifiers, so a later reference-product change cannot silently change the v1
inventory.

The manifest is a release gate, not a product configuration file and not an
availability claim. `unavailable`, `internal`, and `public` are deliberately
distinct. Full parity may be claimed only when `parity_claim` is true, every
required capability is `public`, its owning gate and dependencies are
`verified`, and each public capability carries test evidence.

The strict parser and inventory live in `a3s-cloud-contracts`. The integration
tests reject schema drift, missing or duplicate inventory entries, noncanonical
ACL, untyped evidence, missing evidence paths, and false public claims.

Architecture decisions are recorded under
[`docs/decisions/app-platform`](../../docs/decisions/app-platform/README.md).
