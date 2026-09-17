# GA-0 / BX0.software engineering checklist

**Status as of 2026-09-17: Verified.**

Authority:
[architecture-optimization-roadmap.md](architecture-optimization-roadmap.md)
(GA-0) and [ROADMAP.md](../ROADMAP.md) (`BX0` software vs tee profiles).

Retained evidence:
[evidence/bx0-software-exit-2026-09-17](evidence/bx0-software-exit-2026-09-17/)
(`EXIT_CERTIFIED profile=software power_revision=UNBOUND`).

## Exit definition

GA-0 is **Verified** only when an operator-owned Linux x86_64 clean host emits:

```text
A3S_CLOUD_BX0_CLEAN_HOST_EXIT_CERTIFIED
  … profile=software power_revision=UNBOUND
  loop=included receipts=included
```

bound to exact Cloud / Runtime / Box / Gateway pins, with matching execute
receipts for enroll → OCI → deploy → health → HTTPS → logs → update →
rollback → stop/cleanup.

**Not required for GA-0:**

- AMD SEV-SNP / `BX0.tee` (`A3S_CLOUD_BX0_TEE_ISOLATION_*`)
- Power pin / `PW0` (`profile=power` is a stronger EXIT)
- Multi-node HA (`H0.3`+)
- Full product verticals (`A1`/`W0`/`APP0` matrices)

## Operator sequence

Follow [OPERATOR_CLEAN_HOST.md](../tools/box-conformance/OPERATOR_CLEAN_HOST.md):

| Step | Command / artifact | Success signal |
| --- | --- | --- |
| 1 | `install_box_release.sh` | Pin-matched `a3s-box` on Linux x86_64 |
| 2 | `run_bx0_clean_host_gate.sh` (armed) | Exit 3 `CLEAN_HOST_OPEN` preflight |
| 3 | Same gate with `A3S_CLOUD_BX0_EXECUTE=1` + real IDs | `execute_receipts_complete=1` |
| 4 | `collect_bx0_clean_host_evidence.sh` | `LOOP_CERTIFIED` file (not example) |
| 5 | `A3S_CLOUD_BX0_EXIT_PROFILE=software` + `run_bx0_clean_host_exit_audit.sh` | `EXIT_CERTIFIED … profile=software` |

CI refuse paths (not product EXIT): `run_bx0_clean_host_gate_ci.sh` /
`run_bx0_clean_host_gate_ci_via_box.sh`.

## Engineering backlog (ordered)

1. **Automate execute steps 1–9** so operators are not hand-feeding every
   receipt env var; keep fail-closed PLACEHOLDER rejection.
   - Landed: `probe_bx0_{logs,digest,rollback}.sh` +
     `run_bx0_software_exit_harness.sh` (CI refuse mode + LIVE orchestration
     that consumes operator-owned real identities; never invents them).
2. **Joint Cloud+Box+Gateway LIVE orchestrator** that *creates* enroll, OCI
   binding, deploy, and (with `CREATE_FULL=1`) logs/update/rollback/cleanup.
   - Landed: `run_bx0_software_loop_create.sh` (fail-closed; requires
     pre-published OCI digest — never invents digests/EXIT/Power).
   - CREATE_FULL captures `observedRuntime.providerResourceId`, proves
     `a3s-box inspect` before stop, `rm --force`, then inspect-absent only
     (rejects workload-id cleanup fallbacks in the gate).
   - Harness opt-in: `A3S_CLOUD_BX0_SOFTWARE_EXIT_CREATE=1` before LIVE;
     one-shot binder: `run_bx0_software_exit_live.sh` (Docker-free only).
   - Still operator-owned: OCI *publish* to a registry, verified domain claim /
     gateway scope (or explicit HTTPS_URL), and Ready health URL.
   - Docker-free stack prep: `run_bx0_software_exit_live_prep.sh`
     (a3s-box compose only; never Docker middleware fallback).
   - Agent release HTTPS + CA-aware health TLS:
     `run_bx0_software_exit_live_agent_release.sh`,
     `run_bx0_software_exit_live_https.sh` (probe uses `--cacert`, not `-k`).
   - One-shot chain: `run_bx0_software_exit_live_chain.sh`
     (prep→tenant→oci→agent→https→EXIT; pin-matched gateway required).
   - Hosts with Docker sock: `run_bx0_software_exit_live_via_box.sh` runs the
     chain in a Docker-free a3s-box guest (no sock override theater on the
     outer host). Canonical `/var/run/docker.sock` is always refused.
   - Still required for Verified: retained `EXIT_CERTIFIED profile=software`
     evidence on a Docker-free guest/host (this checklist host may still be
     blocked by docker.sock until via-box or a clean machine succeeds).
   - **Done (`2026-09-17`):** retained bundle
     [evidence/bx0-software-exit-2026-09-17](evidence/bx0-software-exit-2026-09-17/)
     on a Docker-free Linux host (`NO_DOCKER_SOCK`); ROADMAP `BX0.software`
     marked Verified with exact Cloud/Runtime/Box/Gateway pins.
3. **Retain one public Actions (or operator) evidence bundle** with
   `EXIT_CERTIFIED profile=software` and pin SHAs in ROADMAP `BX0` state.
   - **Done (`2026-09-17`):** operator bundle under
     [evidence/bx0-software-exit-2026-09-17](evidence/bx0-software-exit-2026-09-17/).
4. **Mark ROADMAP `BX0.software` Verified** only after step 3; leave
   `BX0.tee` In progress / blocked on SEV capacity.
   - **Done (`2026-09-17`):** `BX0.software` Verified; `BX0.tee` unchanged.
5. **Then start GA-1** (narrow Code-path Agent availability) — do not start
   `PW0.tee`, distributed `I0`, `CELL0` production, or `EV0` as substitutes.

```bash
# Refuse-to-fake CI (never product EXIT):
bash tools/box-conformance/run_bx0_software_exit_harness_ci.sh

# Assisted LIVE after real enroll→…→cleanup identities are exported:
export A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1
# … export URL/token/org/project/env + BX0_* identities …
bash tools/box-conformance/run_bx0_software_exit_harness.sh

# Or CREATE enroll→deploy→… then EXIT on a clean Linux host:
export A3S_CLOUD_BX0_SOFTWARE_EXIT_LIVE=1
export A3S_CLOUD_BX0_SOFTWARE_EXIT_CREATE=1
export A3S_CLOUD_BX0_CREATE_FULL=1
# … URL/token/org/project/env + node.acl + agent release + pre-published OCI
# … + HEALTH_URL + HTTPS_URL + UPDATE digest/uri …
bash tools/box-conformance/run_bx0_software_exit_harness.sh

# One-shot chain (Docker-free Linux x86_64 only):
bash tools/box-conformance/install_gateway_pin.sh /abs/empty/gw
export A3S_CLOUD_GATEWAY_BIN=/abs/empty/gw/a3s-gateway
bash tools/box-conformance/run_bx0_software_exit_live_chain.sh

# Dirty host (has docker.sock): run chain inside Docker-free MicroVM guest:
export A3S_CLOUD_BOX_BIN=/abs/a3s-box
bash tools/box-conformance/run_bx0_software_exit_live_via_box.sh
```

## Stronger profiles (explicitly later)

| Profile | Extra evidence | Starts after |
| --- | --- | --- |
| `power` | Bound `tools/power-conformance/power-revision` + Power serve path | GA-0 |
| `tee` | `run_bx0_tee_isolation_audit.sh` + green Box SEV-SNP job | Online `sev-snp` runner |
| `ha` | `H0.3`–`H0.5` cluster gates | GA-0 + ≥1 product vertical |

## Honesty rules

- Skipped SEV jobs are not Verified tee and must not freeze GA-0.
- Module folders and component-only slices are not GA-0.
- `EXIT_CERTIFIED profile=power` must not be marketed as tee.
- Inventing `tools/power-conformance/power-revision` in CI is forbidden.
