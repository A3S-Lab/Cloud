# BX0.3 hardware TEE isolation operator guide (does not claim Verified)

Product BX0 Verified stays open until:

1. Retained Box provider sandbox/conformance evidence remains green for the
   pinned Box revision, and
2. Box job `Integration (hardware SEV-SNP)` is **success** on that same pin
   (`simulate=false` only), and
3. Cloud binds that run through the TEE isolation audit below.

Sandbox re-cert, KVM-only MicroVM gates, skipped SEV jobs, WSL, and
`simulate=true` are **not** hardware TEE evidence.

## Org prerequisites (Box)

Admin inventory preflight (does **not** set variables):

```bash
bash tools/box-conformance/preflight_bx0_sev_org_arming.sh
# expected while unarmed: exit 2 A3S_CLOUD_BX0_SEV_ORG_ARMING_BLOCKED
```

As of the Cloud BX0 tip that introduced this preflight, `A3S-Lab/Box` had
**zero** repo self-hosted runners and **zero** Actions variables, so
`Integration (hardware SEV-SNP)` stays skipped. Do **not** set
`SEV_SNP_CI=true` until an online runner carries the `sev-snp` label — arming
without capacity only queues a job that cannot certify.

Intel/WSL hosts with `/dev/kvm` but no `/dev/sev` cannot be `sev-snp` runners.

Follow Box `docs/ci-kvm-runner.md` on the pinned Box revision:

1. Register a trusted AMD SEV-SNP host as a self-hosted runner with labels:
   `self-hosted`, `linux`, `kvm`, `sev-snp`
2. Runner user can access `/dev/kvm` and `/dev/sev`
3. `/sys/module/kvm_amd/parameters/sev_snp` reports `Y` or `1`
4. Repository variables on `A3S-Lab/Box`:
   - `SEV_SNP_CI=true`
   - `SEV_SNP_CI_GENERATION=milan` or `genoa`
   - `SEV_SNP_CI_EXPECTED_MEASUREMENT` = 96 lowercase hex (SHA-384 launch measurement)

Pull requests skip the hardware job by design. Arm on `main` / dispatch against
the pinned tip, then wait for **success** (not skipped).

## Cloud bind (after the Box job is green)

From this Cloud checkout (exact tip + `tools/box-conformance/box-revision`):

```bash
# Refuse skipped / SHA-mismatched runs; writes TEE_ISOLATION_CERTIFIED only on success.
bash tools/box-conformance/collect_bx0_tee_isolation_evidence.sh \
  --box-hardware-sev-run https://github.com/A3S-Lab/Box/actions/runs/<id> \
  --generation milan \
  --expected-measurement <96hex> \
  --evidence-dir /tmp/bx0-tee-evidence

export A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFICATION=/tmp/bx0-tee-evidence/bx0-tee-isolation-certification.txt
bash tools/box-conformance/run_bx0_tee_isolation_audit.sh /tmp/bx0-tee-audit
# expected only with valid green hardware evidence:
#   A3S_CLOUD_BX0_TEE_ISOLATION_CERTIFIED ...
# without evidence:
#   exit 2 A3S_CLOUD_BX0_TEE_ISOLATION_BLOCKED
```

The audit re-checks the Box Actions job remotely when `gh` can read
`A3S-Lab/Box` (fail-closed on skipped/mismatched runs even if a hand-edited
cert file claims success).

## CI harness (does not unlock Verified)

```bash
bash tools/box-conformance/run_bx0_tee_isolation_audit_ci.sh
```

## Still blocked after TEE

- `PW0` / Power pin
- Product `A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED` (needs LOOP + Power)

**Physical-capacity stop:** if org SEV arming is blocked (no online `sev-snp`
runner / no `/dev/sev` host), **skip developing** those subsequent features.
Do not stub Power, fake EXIT, or advance I0 Track B to "make progress." Resume
only after hardware TEE isolation evidence is bound.

