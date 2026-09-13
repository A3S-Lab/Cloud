#!/usr/bin/env bash
# Fail-closed preflight for Box hardware SEV-SNP org arming.
# Reports runner/variable inventory. Never sets SEV_SNP_CI.
# Exit 0 only when an online runner carries label sev-snp.
# Exit 2 when hardware capacity is missing.
#
# Usage:
#   bash tools/box-conformance/preflight_bx0_sev_org_arming.sh

set -euo pipefail

if ! command -v gh >/dev/null 2>&1; then
  printf '%s\n' "gh CLI required" >&2
  exit 1
fi

total=$(gh api repos/A3S-Lab/Box/actions/runners --jq .total_count)
printf 'box_self_hosted_runners_total=%s\n' "$total"

gh api repos/A3S-Lab/Box/actions/runners --jq '
  .runners[]?
  | {
      name,
      status,
      busy,
      sev_snp: ([.labels[].name] | index("sev-snp") != null),
      labels: ([.labels[].name] | join(","))
    }
  | "runner name=\(.name) status=\(.status) busy=\(.busy) sev_snp=\(.sev_snp) labels=\(.labels)"
'

sev_online=$(
  gh api repos/A3S-Lab/Box/actions/runners --jq '
    [
      .runners[]?
      | select(([.labels[].name] | index("sev-snp") != null) and .status == "online")
      | .name
    ]
    | length
  '
)
printf 'online_sev_snp_runners=%s\n' "$sev_online"

var_total=$(gh api repos/A3S-Lab/Box/actions/variables --jq .total_count)
printf 'box_actions_variables_total=%s\n' "$var_total"
gh api repos/A3S-Lab/Box/actions/variables --jq '.variables[]? | "variable name=\(.name)"'

names=$(gh api repos/A3S-Lab/Box/actions/variables --jq '[.variables[]?.name] | join(" ")')
for want in SEV_SNP_CI SEV_SNP_CI_GENERATION SEV_SNP_CI_EXPECTED_MEASUREMENT; do
  present=false
  for name in $names; do
    if [[ $name == "$want" ]]; then
      present=true
      break
    fi
  done
  printf 'variable_present name=%s present=%s\n' "$want" "$present"
done

if [[ $sev_online -gt 0 ]]; then
  printf '%s\n' \
    "A3S_CLOUD_BX0_SEV_ORG_ARMING_READY" \
    "Online sev-snp runner present. Set Box variables only with the real" \
    "generation/measurement for that host, then dispatch CI on the pinned tip." \
    "Do not use simulate=true. See OPERATOR_TEE.md."
  exit 0
fi

printf '%s\n' \
  "A3S_CLOUD_BX0_SEV_ORG_ARMING_BLOCKED reason=no_online_sev_snp_runner" \
  "Refuse setting SEV_SNP_CI=true until an AMD SEV-SNP host is registered with" \
  "labels self-hosted,linux,kvm,sev-snp and shows status=online." \
  "This preflight never arms variables." >&2
exit 2
