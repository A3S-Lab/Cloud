# GA-1 Gateway LIVE attempt (2026-09-18) — BLOCKED

Honest fail-closed stop. Middleware and pin-matched gateway were prepared on
OrbStack amd64 guest `a3s-ga1-live`, but `a3s-box` warm-pool / Agent VMs cannot
boot without `/dev/kvm`. OrbStack on Apple Silicon does not expose nested KVM.

## What was proven locally

- Pin-matched Box install `BOX-REVISION=8b2804c…`
- Native Postgres on `:54320` + distribution registry on `:50020` (Hub unreachable)
- Pin-matched `a3s-gateway` built from Gateway@`e9289676…`
- Agent runtime OCI published at digest in `.cache/a0-4/a0-4-image.env` (host Docker bake; **not** A0.4 Box-build CERTIFIED)

## What was not claimed

- No `A3S_CLOUD_GA1_GATEWAY_LIVE_CODE_PATH_CERTIFIED`
- No ROADMAP GA-1 Verified
- Docker-built agent image is not a substitute for A0.4 real-Box CERTIFIED

## Unblock

Run OPERATOR_GA1.md on Linux x86_64 with `/dev/kvm` (bare metal or nested-capable
hypervisor), Docker-free, pin-matched Box + published A0.4 image + Postgres/registry.
