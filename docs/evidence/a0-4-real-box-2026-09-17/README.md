# A0.4 real Box Agent release evidence (`2026-09-17`)

Operator-retained LIVE re-bind of the published Agent release gate on current
`BX0.software` Box pin (closes the `65f3d3fc…` pin-skew finding).

## Certification line

```text
A3S_CLOUD_A0_4_REAL_BOX_RELEASE_CERTIFIED store=postgresql release=published
  deployment_flow=completed stop_flow=completed runtime_apply=persisted
  provider=a3s-box harness=a3s-code observations=3 process_restarts=1
  secret_materializations=4 artifact_downloads=1 commands=5 acknowledgements=5
  cleanup=removed
  artifact_digest=sha256:34cfe15ffa81bcd2d75f5e9b72ed5f3dd8ac1a6e9dd83e0b62f57ab30e7ecda1
  manifest_identity=sha256:20fc393096a413d561a8755d077ffe17fe3a3881d14474c290571ed9874bb691
```

## Pins

| Component | Revision |
| --- | --- |
| Box | `8b2804c585d2f31bce06f213990407606a475634` (`tools/box-conformance/box-revision`) |
| Agent runtime image | `127.0.0.1:50020/a3s/a0-4-agent-runtime@sha256:34cfe15ffa81bcd2d75f5e9b72ed5f3dd8ac1a6e9dd83e0b62f57ab30e7ecda1` |

## Honesty

- Docker-free host path (`a3s-box` only for postgres middleware; host-side
  distribution registry on `:50020` for OCI publish/pull).
- Postgres guest uses compose `tmpfs = ["/tmp/pgdata"]` so initdb can chmod
  (virtiofs/overlay rejects chmod on ordinary guest `/tmp`).
- Does **not** claim Gateway public Agent traffic (GA-1 Gateway LIVE smoke is
  separate). Does **not** claim TEE.

## Bundle layout

| Path | Contents |
| --- | --- |
| `agent-runtime-release-certification.txt` | CERTIFIED marker line |
| `agent-runtime-release.log` | Full ignored-test harness log |
| `a0-4-image.env` | Exact published image digest + media type + size |
| `agent-runtime-image.txt` / `platform-manifest.json` | Publish artifacts |
| `github-fast.env` | Sandbox CI cgroup exports from the run |
| `RETAINED.txt` | Retention metadata |

Tokens and secrets were not retained.
